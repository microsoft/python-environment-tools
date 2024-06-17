// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, info, trace, warn};
use pet_conda::Conda;
use pet_core::arch::Architecture;
use pet_core::os_environment::{Environment, EnvironmentApi};
use pet_core::python_environment::{PythonEnvironmentBuilder, PythonEnvironmentCategory};
use pet_core::reporter::Reporter;
use pet_core::Locator;
use pet_env_var_path::get_search_paths_from_env_variables;
use pet_global_virtualenvs::list_global_virtual_envs_paths;
use pet_mac_commandlinetools::MacCmdLineTools;
use pet_mac_python_org::MacPythonOrg;
use pet_pipenv::PipEnv;
use pet_pyenv::PyEnv;
use pet_python_utils::env::{PythonEnv, ResolvedPythonEnv};
use pet_python_utils::executable::{find_executable, find_executables};
use pet_python_utils::version;
use pet_venv::Venv;
use pet_virtualenv::VirtualEnv;
use pet_virtualenvwrapper::VirtualEnvWrapper;
use std::fs;
use std::ops::Deref;
use std::path::PathBuf;
use std::{sync::Arc, thread};

#[derive(Debug, Default, Clone)]
pub struct Configuration {
    pub search_paths: Option<Vec<PathBuf>>,
    pub conda_executable: Option<PathBuf>,
}

pub fn find_and_report_envs(
    reporter: &dyn Reporter,
    conda_locator: Arc<Conda>,
    configuration: Configuration,
) {
    info!("Started Refreshing Environments");

    let conda_locator1 = conda_locator.clone();
    let conda_locator2 = conda_locator.clone();
    let conda_locator3 = conda_locator.clone();
    let search_paths = configuration.search_paths.unwrap_or_default();
    // 1. Find using known global locators.
    thread::scope(|s| {
        s.spawn(|| {
            find_using_global_finders(conda_locator1, reporter);
        });
        // Step 2: Search in some global locations for virtual envs.
        s.spawn(|| find_in_global_virtual_env_dirs(reporter));
        // Step 3: Finally find in the current PATH variable
        s.spawn(|| {
            let environment = EnvironmentApi::new();
            find_python_environments(
                conda_locator2,
                get_search_paths_from_env_variables(&environment),
                reporter,
                false,
            )
        });
        // Step 4: Find in workspace folders
        s.spawn(|| {
            if search_paths.is_empty() {
                return;
            }
            trace!(
                "Searching for environments in custom folders: {:?}",
                search_paths
            );
            find_python_environments_in_workspace_folders_recursive(
                conda_locator3,
                search_paths,
                reporter,
                0,
                1,
            );
        });
    });
}

#[cfg(windows)]
fn find_using_global_finders(conda_locator: Arc<Conda>, reporter: &dyn Reporter) {
    // Step 1: These environments take precedence over all others.
    // As they are very specific and guaranteed to be specific type.
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
fn find_using_global_finders(conda_locator: Arc<Conda>, reporter: &dyn Reporter) {
    // Step 1: These environments take precedence over all others.
    // As they are very specific and guaranteed to be specific type.

    thread::scope(|s| {
        // The order matters,
        // pyenv can be treated as a virtualenvwrapper environment, hence virtualenvwrapper needs to be detected first
        // Homebrew can happen anytime
        // Conda is best done last, as pyenv can also contain conda envs,
        // Thus lets leave the generic conda locator to last to find all remaining conda envs.

        use pet_homebrew::Homebrew;

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
        // 5. Mac Global Python & CommandLineTools Python (xcode)
        s.spawn(move || {
            if std::env::consts::OS == "macos" {
                MacCmdLineTools::new().find(reporter);
                MacPythonOrg::new().find(reporter);
            }
        });
    });
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
    let pipenv_locator = PipEnv::from(&environment);
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
            // Never use pyvenv.cfg, as this isn't accurate.
            env.version = version::from_header_files(&env_path);

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
                if let Some(env) = locator.from(&env) {
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

fn find_python_environments_in_workspace_folders_recursive(
    conda_locator: Arc<Conda>,
    paths: Vec<PathBuf>,
    reporter: &dyn Reporter,
    depth: u32,
    max_depth: u32,
) {
    thread::scope(|s| {
        // Find in cwd
        let conda_locator1 = conda_locator.clone();
        let paths1 = paths.clone();
        s.spawn(|| {
            find_python_environments(conda_locator1, paths1, reporter, true);

            if depth >= max_depth {
                return;
            }

            let bin = if cfg!(windows) { "Scripts" } else { "bin" };
            // if this is bin or scripts, then we should not go into it.
            // This is because the parent of this would have been discovered above.
            let paths = paths
                .into_iter()
                .filter(|p| !p.join(bin).exists())
                .collect::<Vec<PathBuf>>();

            for path in paths {
                let path = path.clone();
                let conda_locator2 = conda_locator.clone();
                if let Ok(reader) = fs::read_dir(&path) {
                    let reader = reader
                        .filter_map(Result::ok)
                        .map(|p| p.path())
                        .filter(|p| p.is_dir());

                    // Take a batch of 20 items at a time.
                    let reader = reader.fold(vec![], |f, a| {
                        let mut f = f;
                        if f.is_empty() {
                            f.push(vec![a]);
                            return f;
                        }
                        let last_item = f.last_mut().unwrap();
                        if last_item.is_empty() || last_item.len() < 20 {
                            last_item.push(a);
                            return f;
                        }
                        f.push(vec![a]);
                        f
                    });

                    for entry in reader {
                        find_python_environments_in_workspace_folders_recursive(
                            conda_locator2.clone(),
                            entry,
                            reporter,
                            depth + 1,
                            max_depth,
                        );
                    }
                }
            }
        });
    });
}

fn find_python_environments(
    conda_locator: Arc<Conda>,
    paths: Vec<PathBuf>,
    reporter: &dyn Reporter,
    is_workspace_folder: bool,
) {
    if paths.is_empty() {
        return;
    }
    thread::scope(|s| {
        // Step 1: These environments take precedence over all others.
        // As they are very specific and guaranteed to be specific type.

        let environment = EnvironmentApi::new();
        let virtualenv_locator = VirtualEnv::new();
        let venv_locator = Venv::new();
        let virtualenvwrapper = VirtualEnvWrapper::from(&environment);
        let pipenv_locator = PipEnv::from(&environment);

        let mut all_locators: Vec<Arc<dyn Locator>> = vec![];

        // First check if this is a known
        // 1. Windows store Python
        // 2. or Windows registry python
        // Note: If we're looking in workspace folders, we should not look in the registry or store.
        // As its impossible for windows store or registry exes to be in workspace folders.
        if !is_workspace_folder && cfg!(windows) {
            #[cfg(windows)]
            use pet_windows_registry::WindowsRegistry;
            #[cfg(windows)]
            use pet_windows_store::WindowsStore;
            #[cfg(windows)]
            all_locators.push(Arc::new(WindowsStore::from(&environment)));
            #[cfg(windows)]
            let conda_locator1 = conda_locator.clone();
            #[cfg(windows)]
            all_locators.push(Arc::new(WindowsRegistry::from(conda_locator1)))
        }
        // 3. Check if this is Pyenv Python
        let conda_locator1 = conda_locator.clone();
        all_locators.push(Arc::new(PyEnv::from(&environment, conda_locator1)));
        // 4. Check if this is Homebrew Python
        // Note: If we're looking in workspace folders, we should not look in the registry or store.
        // As its impossible for windows store or registry exes to be in workspace folders.
        if !is_workspace_folder && cfg!(unix) {
            #[cfg(unix)]
            use pet_homebrew::Homebrew;
            #[cfg(unix)]
            let homebrew_locator = Homebrew::from(&environment);
            #[cfg(unix)]
            all_locators.push(Arc::new(homebrew_locator));
        }
        // 5. Check if this is Conda Python
        all_locators.push(conda_locator);
        // 6. Finally check if this is some kind of a virtual env
        all_locators.push(Arc::new(pipenv_locator));
        all_locators.push(Arc::new(virtualenvwrapper));
        all_locators.push(Arc::new(venv_locator));
        all_locators.push(Arc::new(virtualenv_locator));
        // Note: If we're looking in workspace folders, then no point trying to identify a
        // Workspace environment as a global one
        if !is_workspace_folder && std::env::consts::OS == "macos" {
            // 7. Possible this is some global Mac Python environment.
            all_locators.push(Arc::new(MacCmdLineTools::new()));
            all_locators.push(Arc::new(MacPythonOrg::new()));
        }
        let all_locators = Arc::new(all_locators);
        let chunks = if is_workspace_folder { paths.len() } else { 1 };
        for item in paths.chunks(chunks) {
            let lst = item.to_vec().clone();
            let all_locators = all_locators.clone();
            s.spawn(move || {
                find_python_environments_in_paths_with_locators(
                    lst,
                    all_locators,
                    reporter,
                    is_workspace_folder,
                );
            });
        }
    });
}

fn find_python_environments_in_paths_with_locators(
    paths: Vec<PathBuf>,
    all_locators: Arc<Vec<Arc<dyn Locator>>>,
    reporter: &dyn Reporter,
    is_workspace_folder: bool,
) {
    let executables = if is_workspace_folder {
        // If we're in a workspace folder, then we only need to look for python or python.exe
        // As this is most likely a virtual env or conda env or the like.
        paths
            .iter()
            // Paths like /Library/Frameworks/Python.framework/Versions/3.10/bin can end up in the current PATH variable.
            // Hence do not just look for files in a bin directory of the path.
            .flat_map(|p| find_executable(p))
            .filter_map(Option::Some)
            .collect::<Vec<PathBuf>>()
    } else {
        paths
            .iter()
            // Paths like /Library/Frameworks/Python.framework/Versions/3.10/bin can end up in the current PATH variable.
            // Hence do not just look for files in a bin directory of the path.
            .flat_map(find_executables)
            .filter(|p| {
                // Exclude python2 on macOS
                if std::env::consts::OS == "macos" {
                    return p.to_str().unwrap_or_default() != "/usr/bin/python2";
                }
                true
            })
            .collect::<Vec<PathBuf>>()
    };

    for exe in executables.into_iter() {
        let executable = exe.clone();
        let env = PythonEnv::new(exe, None, None);
        let locators = all_locators.as_ref().deref();
        if let Some(env) = locators
            .iter()
            .fold(None, |e, loc| if e.is_some() { e } else { loc.from(&env) })
        {
            reporter.report_environment(&env);
            continue;
        }

        // Yikes, we have no idea what this is.
        // Lets get the actual interpreter info and try to figure this out.
        // We try to get the interpreter info, hoping that the real exe returned might be identifiable.
        if let Some(resolved_env) = ResolvedPythonEnv::from(&executable) {
            let env = resolved_env.to_python_env();
            if let Some(env) =
                locators
                    .iter()
                    .fold(None, |e, loc| if e.is_some() { e } else { loc.from(&env) })
            {
                trace!(
                    "Unknown Env ({:?}) in Path resolved as {:?}",
                    executable,
                    env.category
                );
                // TODO: Telemetry point.
                // As we had to spawn earlier.
                reporter.report_environment(&env);
            } else {
                // We have no idea what this is.
                // We have check all of the resolvers.
                // Telemetry point, failed to identify env here.
                warn!(
                    "Unknown Env ({:?}) in Path resolved as {:?} and reported as Unknown",
                    executable, resolved_env
                );
                let env = PythonEnvironmentBuilder::new(PythonEnvironmentCategory::Unknown)
                    .executable(Some(resolved_env.executable))
                    .prefix(Some(resolved_env.prefix))
                    .arch(Some(if resolved_env.is64_bit {
                        Architecture::X64
                    } else {
                        Architecture::X86
                    }))
                    .version(Some(resolved_env.version))
                    .build();
                reporter.report_environment(&env);
            }
        }
    }
}
