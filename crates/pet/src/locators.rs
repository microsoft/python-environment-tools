// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, info, warn};
use pet_conda::Conda;
use pet_core::os_environment::{Environment, EnvironmentApi};
use pet_core::python_environment::PythonEnvironment;
use pet_core::reporter::Reporter;
use pet_core::{Locator, LocatorResult};
use pet_global_virtualenvs::list_global_virtual_envs_paths;
use pet_homebrew::Homebrew;
use pet_pipenv::PipEnv;
use pet_pyenv::PyEnv;
use pet_utils::env::PythonEnv;
use pet_utils::executable::find_executable;
use pet_utils::sys_prefix::SysPrefix;
use pet_venv::Venv;
use pet_virtualenv::VirtualEnv;
use pet_virtualenvwrapper::VirtualEnvWrapper;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::SystemTime;
use std::{sync::Arc, thread};

pub fn find_and_report_envs(reporter: &dyn Reporter) {
    info!("Started Refreshing Environments");
    let now = SystemTime::now();

    // 1. Find using known global locators.
    let mut threads = find_using_global_finders();

    // // Step 2: Search in some global locations for virtual envs.
    threads.push(thread::spawn(find_in_global_virtual_env_dirs));

    // Step 3: Finally find in the current PATH variable
    // threads.push(thread::spawn(find_in_path_env_variable));

    // NOTE: Ensure we process the results in the same order as they were started.
    // This will ensure the priority order is maintained.
    for handle in threads {
        match handle.join() {
            Ok(result) => report_result(result, reporter),
            Err(err) => error!("One of the finders failed. {:?}", err),
        }
    }

    match now.elapsed() {
        Ok(elapsed) => {
            info!("Refreshed Environments in {}ms.", elapsed.as_millis());
        }
        Err(e) => {
            error!("Error getting elapsed time: {:?}", e);
        }
    }
}

fn find_using_global_finders(// dispatcher: &mut dyn MessageDispatcher,
) -> Vec<JoinHandle<Option<LocatorResult>>> {
    // Step 1: These environments take precedence over all others.
    // As they are very specific and guaranteed to be specific type.
    #[cfg(windows)]
    fn find() -> Vec<JoinHandle<std::option::Option<LocatorResult>>> {
        // The order matters,
        // Windows store can sometimes get detected via registry locator (but we want to avoid that),
        //  difficult to repro, but we have see this on Karthiks machine
        // Windows registry can contain conda envs (e.g. installing Ananconda will result in registry entries).
        // Conda is best done last, as Windows Registry and Pyenv can also contain conda envs,
        // Thus lets leave the generic conda locator to last to find all remaining conda envs.
        // pyenv can be treated as a virtualenvwrapper environment, hence virtualenvwrapper needs to be detected first
        vec![
            // // 1. windows store
            // thread::spawn(|| {
            //     let environment = EnvironmentApi::new();
            //     let mut windows_store = windows_store::WindowsStore::with(&environment);
            //     windows_store.find()
            // }),
            // // 2. windows registry
            // thread::spawn(|| {
            //     let environment = EnvironmentApi::new();
            //     let mut conda_locator = conda::Conda::with(&environment);
            //     windows_registry::WindowsRegistry::with(&mut conda_locator).find()
            // }),
            // // 3. virtualenvwrapper
            // thread::spawn(|| {
            //     let environment = EnvironmentApi::new();
            //     virtualenvwrapper::VirtualEnvWrapper::with(&environment).find()
            // }),
            // // 4. pyenv
            // thread::spawn(|| {
            //     let environment = EnvironmentApi::new();
            //     let mut conda_locator = conda::Conda::with(&environment);
            //     pyenv::PyEnv::with(&environment, &mut conda_locator).find()
            // }),
            // // 5. conda
            // thread::spawn(|| {
            //     let environment = EnvironmentApi::new();
            //     conda::Conda::with(&environment).find()
            // }),
        ]
    }

    #[cfg(unix)]
    fn find() -> Vec<JoinHandle<std::option::Option<LocatorResult>>> {
        // The order matters,
        // pyenv can be treated as a virtualenvwrapper environment, hence virtualenvwrapper needs to be detected first
        // Homebrew can happen anytime
        // Conda is best done last, as pyenv can also contain conda envs,
        // Thus lets leave the generic conda locator to last to find all remaining conda envs.

        use pet_virtualenvwrapper::VirtualEnvWrapper;

        let environment = EnvironmentApi::new();
        let conda_locator = Arc::new(Conda::from(&environment));
        let conda_locator1 = conda_locator.clone();
        let conda_locator2 = conda_locator.clone();
        vec![
            // 1. virtualenvwrapper
            thread::spawn(|| {
                let environment = EnvironmentApi::new();
                VirtualEnvWrapper::from(&environment).find()
            }),
            // 2. pyenv
            thread::spawn(|| {
                let environment = EnvironmentApi::new();
                PyEnv::from(&environment, conda_locator1).find()
            }),
            // 3. homebrew
            thread::spawn(|| {
                let environment = EnvironmentApi::new();
                Homebrew::from(&environment).find()
            }),
            // 4. conda
            thread::spawn(move || conda_locator2.find()),
        ]
    }

    find()
}

fn find_in_global_virtual_env_dirs() -> Option<LocatorResult> {
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
    let mut environments = Vec::<PythonEnvironment>::new();
    for env_path in envs_from_global_locations {
        if let Some(executable) = find_executable(&env_path) {
            let mut env = PythonEnv::new(executable.clone(), Some(env_path.clone()), None);

            // Try to get the version from the env directory
            env.version = SysPrefix::get_version(&env_path);

            // 1. First must be homebrew, as it is the most specific and supports symlinks
            #[cfg(unix)]
            if let Some(env) = homebrew_locator.from(&env) {
                environments.push(env);
                continue;
            }

            // 3. Finally Check if these are some kind of virtual env or pipenv.
            // Pipeenv before virtualenvwrapper as it is more specific.
            // Because pipenv environments are also virtualenvwrapper environments.
            // Before venv, as all venvs are also virtualenvwrapper environments.
            // Before virtualenv as this is more specific.
            // All venvs are also virtualenvs environments.
            for locator in &venv_type_locators {
                if let Some(env) = locator.as_ref().from(&env) {
                    environments.push(env);
                    break;
                } else {
                    // We have no idea what this is.
                    // Lets keep track of this and we can resolve this later.
                    warn!("Unknown environment: {:?}", env);
                }
            }
        }
    }
    Some(LocatorResult {
        environments,
        managers: vec![],
    })
}

// This is incomplete
// fn find_in_path_env_variable() -> Option<LocatorResult> {
//     let environment = EnvironmentApi::new();
//     PythonOnPath::from(&environment).find()
// }

fn report_result(result: Option<LocatorResult>, reporter: &dyn Reporter) {
    if let Some(result) = result {
        result
            .environments
            .iter()
            .for_each(|e| reporter.report_environment(e));
        result
            .managers
            .iter()
            .for_each(|m| reporter.report_manager(m));
    }
}
