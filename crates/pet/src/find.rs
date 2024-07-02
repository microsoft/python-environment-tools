// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{info, trace, warn};
use pet_conda::CondaLocator;
use pet_core::os_environment::{Environment, EnvironmentApi};
use pet_core::python_environment::PythonEnvironmentCategory;
use pet_core::reporter::Reporter;
use pet_core::{Configuration, Locator};
use pet_env_var_path::get_search_paths_from_env_variables;
use pet_global_virtualenvs::list_global_virtual_envs_paths;
use pet_poetry::Poetry;
use pet_python_utils::env::PythonEnv;
use pet_python_utils::executable::{
    find_executable, find_executables, should_search_for_environments_in_path,
};
use pet_reporter::cache::{CacheReporter, CacheReporterImpl};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use std::{sync::Arc, thread};

use crate::locators::identify_python_environment_using_locators;

pub struct Summary {
    pub time: Duration,
    pub find_locators_times: BTreeMap<&'static str, Duration>,
    pub find_locators_time: Duration,
    pub find_path_time: Duration,
    pub find_global_virtual_envs_time: Duration,
    pub find_search_paths_time: Duration,
}

pub fn find_and_report_envs(
    reporter: &CacheReporterImpl,
    configuration: Configuration,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    conda_locator: Arc<dyn CondaLocator>,
) -> Arc<Mutex<Summary>> {
    let summary = Arc::new(Mutex::new(Summary {
        time: Duration::from_secs(0),
        find_locators_times: BTreeMap::new(),
        find_locators_time: Duration::from_secs(0),
        find_path_time: Duration::from_secs(0),
        find_global_virtual_envs_time: Duration::from_secs(0),
        find_search_paths_time: Duration::from_secs(0),
    }));
    info!("Started Refreshing Environments");
    let start = std::time::Instant::now();

    // From settings
    let environment_paths = configuration.environment_paths.unwrap_or_default();
    let search_paths = configuration.search_paths.unwrap_or_default();
    let conda_executable = configuration.conda_executable;
    thread::scope(|s| {
        // 1. Find using known global locators.
        s.spawn(|| {
            // Find in all the finders
            let start = std::time::Instant::now();
            thread::scope(|s| {
                for locator in locators.iter() {
                    let locator = locator.clone();
                    let summary = summary.clone();
                    s.spawn(move || {
                        let start = std::time::Instant::now();
                        locator.find(reporter);
                        // locator.find(&Box::new(reporter.clone()) as &Box<dyn Reporter>);
                        summary
                            .lock()
                            .unwrap()
                            .find_locators_times
                            .insert(locator.get_name(), start.elapsed());
                    });
                }
            });
            summary.lock().unwrap().find_locators_time = start.elapsed();

            // By now all conda envs have been found
            // Spawn conda  in a separate thread.
            // & see if we can find more environments by spawning conda.
            // But we will not wait for this to complete.
            thread::spawn(move || {
                conda_locator.find_with_conda_executable(conda_executable);
                Some(())
            });
            // By now all poetry envs have been found
            // Spawn poetry exe in a separate thread.
            // & see if we can find more environments by spawning poetry.
            // But we will not wait for this to complete.
            thread::spawn(move || {
                let env = EnvironmentApi::new();
                Poetry::new(&env).find_with_executable();
                Some(())
            });
        });
        // Step 2: Search in PATH variable
        s.spawn(|| {
            let start = std::time::Instant::now();
            let environment = EnvironmentApi::new();
            let search_paths: Vec<PathBuf> = get_search_paths_from_env_variables(&environment);

            trace!(
                "Searching for environments in global folders: {:?}",
                search_paths
            );
            find_python_environments(
                search_paths,
                reporter,
                locators,
                false,
                Some(PythonEnvironmentCategory::GlobalPaths),
            );
            summary.lock().unwrap().find_path_time = start.elapsed();
        });
        // Step 3: Search in some global locations for virtual envs.
        s.spawn(|| {
            let start = std::time::Instant::now();
            let environment = EnvironmentApi::new();
            let search_paths: Vec<PathBuf> = [
                list_global_virtual_envs_paths(
                    environment.get_env_var("WORKON_HOME".into()),
                    environment.get_user_home(),
                ),
                environment_paths,
            ]
            .concat();

            trace!(
                "Searching for environments in global venv folders: {:?}",
                search_paths
            );

            find_python_environments(search_paths, reporter, locators, false, None);
            summary.lock().unwrap().find_global_virtual_envs_time = start.elapsed();
        });
        // Step 4: Find in workspace folders too.
        // This can be merged with step 2 as well, as we're only look for environments
        // in some folders.
        // However we want step 2 to happen faster, as that list of generally much smaller.
        // This list of folders generally map to workspace folders
        // & users can have a lot of workspace folders and can have a large number fo files/directories
        // that could the discovery.
        s.spawn(|| {
            if search_paths.is_empty() {
                return;
            }
            trace!(
                "Searching for environments in custom folders: {:?}",
                search_paths
            );
            let start = std::time::Instant::now();
            find_python_environments_in_workspace_folders_recursive(
                search_paths,
                reporter,
                locators,
                0,
                1,
            );
            summary.lock().unwrap().find_search_paths_time = start.elapsed();
        });
    });
    summary.lock().unwrap().time = start.elapsed();

    summary
}

fn find_python_environments_in_workspace_folders_recursive(
    paths: Vec<PathBuf>,
    reporter: &CacheReporterImpl,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    depth: u32,
    max_depth: u32,
) {
    thread::scope(|s| {
        // Find in cwd
        let paths1 = paths.clone();
        s.spawn(|| {
            find_python_environments(paths1, reporter, locators, true, None);

            if depth >= max_depth {
                return;
            }

            let bin = if cfg!(windows) { "Scripts" } else { "bin" };
            // If the folder has a bin or scripts, then ignore it, its most likely an env.
            // I.e. no point looking for python environments in a Python environment.
            let paths = paths
                .into_iter()
                .filter(|p| !p.join(bin).exists())
                .collect::<Vec<PathBuf>>();

            for path in paths {
                if let Ok(reader) = fs::read_dir(&path) {
                    let reader = reader
                        .filter_map(Result::ok)
                        .filter(|d| d.file_type().is_ok_and(|f| f.is_dir()))
                        .map(|p| p.path())
                        .filter(should_search_for_environments_in_path);

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
                            entry,
                            reporter,
                            locators,
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
    paths: Vec<PathBuf>,
    reporter: &CacheReporterImpl,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    is_workspace_folder: bool,
    fallback_category: Option<PythonEnvironmentCategory>,
) {
    if paths.is_empty() {
        return;
    }
    thread::scope(|s| {
        let chunks = if is_workspace_folder { paths.len() } else { 1 };
        for item in paths.chunks(chunks) {
            let lst = item.to_vec().clone();
            let locators = locators.clone();
            s.spawn(move || {
                find_python_environments_in_paths_with_locators(
                    lst,
                    &locators,
                    reporter,
                    is_workspace_folder,
                    fallback_category,
                );
            });
        }
    });
}

fn find_python_environments_in_paths_with_locators(
    paths: Vec<PathBuf>,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    reporter: &CacheReporterImpl,
    is_workspace_folder: bool,
    fallback_category: Option<PythonEnvironmentCategory>,
) {
    let executables = if is_workspace_folder {
        // If we're in a workspace folder, then we only need to look for bin/python or bin/python.exe
        // As workspace folders generally have either virtual env or conda env or the like.
        // They will not have environments that will ONLY have a file like `bin/python3`.
        // I.e. bin/python will almost always exist.
        paths
            .iter()
            // Paths like /Library/Frameworks/Python.framework/Versions/3.10/bin can end up in the current PATH variable.
            // Hence do not just look for files in a bin directory of the path.
            .flat_map(|p| find_executable(p))
            .filter_map(Option::Some)
            .filter(|p| !reporter.was_reported(p))
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
            .filter(|p| !reporter.was_reported(p))
            .collect::<Vec<PathBuf>>()
    };

    identify_python_executables_using_locators(executables, locators, reporter, fallback_category);
}

fn identify_python_executables_using_locators(
    executables: Vec<PathBuf>,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    reporter: &CacheReporterImpl,
    fallback_category: Option<PythonEnvironmentCategory>,
) {
    for exe in executables.into_iter() {
        if reporter.was_reported(&exe) {
            continue;
        }
        let executable = exe.clone();
        let env = PythonEnv::new(exe.to_owned(), None, None);
        if let Some(env) =
            identify_python_environment_using_locators(&env, locators, fallback_category)
        {
            reporter.report_environment(&env);
            if let Some(manager) = env.manager {
                reporter.report_manager(&manager);
            }
            continue;
        } else {
            warn!("Unknown Python Env {:?}", executable);
        }
    }
}
