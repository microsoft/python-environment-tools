// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, info};
use pet_conda::Conda;
use pet_core::os_environment::{Environment, EnvironmentApi};
use pet_core::reporter::Reporter;
use pet_core::Locator;
use pet_global_virtualenvs::list_global_virtual_envs_paths;
use pet_pipenv::PipEnv;
use pet_pyenv::PyEnv;
use pet_utils::env::PythonEnv;
use pet_utils::executable::find_executable;
use pet_utils::sys_prefix::SysPrefix;
use pet_venv::Venv;
use pet_virtualenv::VirtualEnv;
use pet_virtualenvwrapper::VirtualEnvWrapper;
use std::path::PathBuf;
use std::time::SystemTime;
use std::{sync::Arc, thread};

pub fn find_and_report_envs(reporter: &dyn Reporter) {
    info!("Started Refreshing Environments");
    let now = SystemTime::now();
    // let reporter = Arc::new(reporter);

    // 1. Find using known global locators.
    thread::scope(|s| {
        s.spawn(|| find_using_global_finders(reporter));
        // Step 2: Search in some global locations for virtual envs.
        s.spawn(|| find_in_global_virtual_env_dirs(reporter));
        // Step 3: Finally find in the current PATH variable
        // s.spawn(find_in_path_env_variable());
    });

    reporter.report_completion(now.elapsed().unwrap_or_default());
}

fn find_using_global_finders(reporter: &dyn Reporter) {
    // Step 1: These environments take precedence over all others.
    // As they are very specific and guaranteed to be specific type.
    #[cfg(windows)]
    fn find(reporter: &dyn Reporter) {
        thread::scope(|s| {
            use pet_windows_registry::WindowsRegistry;
            use pet_windows_store::WindowsStore;
            // use pet_win
            // The order matters,
            // Windows store can sometimes get detected via registry locator (but we want to avoid that),
            //  difficult to repro, but we have see this on Karthiks machine
            // Windows registry can contain conda envs (e.g. installing Ananconda will result in registry entries).
            // Conda is best done last, as Windows Registry and Pyenv can also contain conda envs,
            // Thus lets leave the generic conda locator to last to find all remaining conda envs.
            // pyenv can be treated as a virtualenvwrapper environment, hence virtualenvwrapper needs to be detected first
            let environment = EnvironmentApi::new();
            let conda_locator = Arc::new(Conda::from(&environment));
            let conda_locator1 = conda_locator.clone();
            let conda_locator2 = conda_locator.clone();
            let conda_locator3 = conda_locator.clone();

            // 1. windows store
            s.spawn(|| {
                let environment = EnvironmentApi::new();
                WindowsStore::from(&environment).find(reporter)
            });
            // 2. windows registry
            s.spawn(|| WindowsRegistry::from(conda_locator1).find(reporter));
            // 3. virtualenvwrapper
            s.spawn(|| {
                let environment = EnvironmentApi::new();
                VirtualEnvWrapper::from(&environment).find(reporter)
            });
            // 4. pyenv
            s.spawn(|| {
                let environment = EnvironmentApi::new();
                PyEnv::from(&environment, conda_locator2).find(reporter)
            });
            // 5. conda
            s.spawn(move || conda_locator3.find(reporter));
        });
    }

    #[cfg(unix)]
    fn find(reporter: &dyn Reporter) {
        thread::scope(|s| {
            // The order matters,
            // pyenv can be treated as a virtualenvwrapper environment, hence virtualenvwrapper needs to be detected first
            // Homebrew can happen anytime
            // Conda is best done last, as pyenv can also contain conda envs,
            // Thus lets leave the generic conda locator to last to find all remaining conda envs.

            use pet_homebrew::Homebrew;

            let environment = EnvironmentApi::new();
            let conda_locator = Arc::new(Conda::from(&environment));
            let conda_locator1 = conda_locator.clone();
            let conda_locator2 = conda_locator.clone();
            // 1. virtualenvwrapper
            s.spawn(|| {
                let environment = EnvironmentApi::new();
                VirtualEnvWrapper::from(&environment).find(reporter)
            });
            // 2. pyenv
            s.spawn(|| {
                let environment = EnvironmentApi::new();
                PyEnv::from(&environment, conda_locator1).find(reporter)
            });
            // 3. homebrew
            s.spawn(|| {
                let environment = EnvironmentApi::new();
                Homebrew::from(&environment).find(reporter)
            });
            // 4. conda
            s.spawn(move || conda_locator2.find(reporter));
        });
    }

    find(reporter)
}

fn find_in_global_virtual_env_dirs(reporter: &dyn Reporter) {
    #[cfg(unix)]
    use pet_homebrew::Homebrew;

    let custom_virtual_env_dirs: Vec<PathBuf> = vec![];

    // Step 1: These environments take precedence over all others.
    // As they are very specific and guaranteed to be specific type.

    let environment = EnvironmentApi::new();
    let virtualenv_locator = VirtualEnv::new();
    let venv_locator = Venv::new();
    let virtualenvwrapper = VirtualEnvWrapper::from(&environment);
    let pipenv_locator = PipEnv::new();
    #[cfg(unix)]
    let homebrew_locator = Homebrew::from(&environment);

    let venv_type_locators = vec![
        Box::new(pipenv_locator) as Box<dyn Locator>,
        Box::new(virtualenvwrapper) as Box<dyn Locator>,
        Box::new(venv_locator) as Box<dyn Locator>,
        Box::new(virtualenv_locator) as Box<dyn Locator>,
    ];

    // Find python envs in custom locations
    let envs_from_global_locations: Vec<PathBuf> = [
        list_global_virtual_envs_paths(
            environment.get_env_var("WORKON_HOME".into()),
            environment.get_user_home(),
        ),
        custom_virtual_env_dirs,
    ]
    .concat();
    // Step 2: Search in some global locations for virtual envs.
    for env_path in envs_from_global_locations {
        if let Some(executable) = find_executable(&env_path) {
            let mut env = PythonEnv::new(executable.clone(), Some(env_path.clone()), None);

            // Try to get the version from the env directory
            env.version = SysPrefix::get_version(&env_path);

            // 1. First must be homebrew, as it is the most specific and supports symlinks
            #[cfg(unix)]
            if let Some(env) = homebrew_locator.from(&env) {
                reporter.report_environment(&env);
                continue;
            }

            // 3. Finally Check if these are some kind of virtual env or pipenv.
            // Pipeenv before virtualenvwrapper as it is more specific.
            // Because pipenv environments are also virtualenvwrapper environments.
            // Before venv, as all venvs are also virtualenvwrapper environments.
            // Before virtualenv as this is more specific.
            // All venvs are also virtualenvs environments.
            let mut found = false;
            for locator in &venv_type_locators {
                if let Some(env) = locator.as_ref().from(&env) {
                    reporter.report_environment(&env);
                    found = true;
                    break;
                }
            }
            if !found {
                // We have no idea what this is.
                // We have check all of the resolvers.
                error!("Unknown Global Virtual Environment: {:?}", env);
            }
        }
    }
}
