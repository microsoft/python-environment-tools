// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{trace, warn};
use pet_core::os_environment::Environment;
use pet_core::reporter::Reporter;
use pet_core::{Configuration, Locator};
use pet_env_var_path::get_search_paths_from_env_variables;
use pet_global_virtualenvs::list_global_virtual_envs_paths;
use pet_python_utils::env::PythonEnv;
use pet_python_utils::executable::{
    find_executable, find_executables, should_search_for_environments_in_path,
};
use pet_venv::is_venv_dir;
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
    reporter: &dyn Reporter,
    configuration: Configuration,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    environment: &dyn Environment,
) -> Arc<Mutex<Summary>> {
    let summary = Arc::new(Mutex::new(Summary {
        time: Duration::from_secs(0),
        find_locators_times: BTreeMap::new(),
        find_locators_time: Duration::from_secs(0),
        find_path_time: Duration::from_secs(0),
        find_global_virtual_envs_time: Duration::from_secs(0),
        find_search_paths_time: Duration::from_secs(0),
    }));
    let start = std::time::Instant::now();

    // From settings
    let environment_directories = configuration.environment_directories.unwrap_or_default();
    let project_directories = configuration.project_directories.unwrap_or_default();
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
                        summary
                            .lock()
                            .unwrap()
                            .find_locators_times
                            .insert(locator.get_name(), start.elapsed());
                    });
                }
            });
            summary.lock().unwrap().find_locators_time = start.elapsed();
        });
        // Step 2: Search in PATH variable
        s.spawn(|| {
            let start = std::time::Instant::now();
            let global_env_search_paths: Vec<PathBuf> =
                get_search_paths_from_env_variables(environment);

            trace!(
                "Searching for environments in global folders: {:?}",
                global_env_search_paths
            );
            find_python_environments(
                global_env_search_paths.clone(),
                reporter,
                locators,
                false,
                &global_env_search_paths,
                None,
            );
            summary.lock().unwrap().find_path_time = start.elapsed();
        });
        // Step 3: Search in some global locations for virtual envs.
        s.spawn(|| {
            let start = std::time::Instant::now();
            let search_paths: Vec<PathBuf> = [
                list_global_virtual_envs_paths(
                    environment.get_env_var("WORKON_HOME".into()),
                    environment.get_env_var("XDG_DATA_HOME".into()),
                    environment.get_user_home(),
                ),
                environment_directories,
            ]
            .concat();
            let global_env_search_paths: Vec<PathBuf> =
                get_search_paths_from_env_variables(environment);

            trace!(
                "Searching for environments in global venv folders: {:?}",
                search_paths
            );

            find_python_environments(
                search_paths,
                reporter,
                locators,
                false,
                &global_env_search_paths,
                None,
            );
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
            if project_directories.is_empty() {
                return;
            }
            trace!(
                "Searching for environments in custom folders: {:?}",
                project_directories
            );
            let start = std::time::Instant::now();
            find_python_environments_in_workspace_folders_recursive(
                project_directories,
                reporter,
                locators,
            );
            summary.lock().unwrap().find_search_paths_time = start.elapsed();
        });
    });
    summary.lock().unwrap().time = start.elapsed();

    summary
}

fn find_python_environments_in_workspace_folders_recursive(
    workspace_folders: Vec<PathBuf>,
    reporter: &dyn Reporter,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
) {
    thread::scope(|s| {
        s.spawn(|| {
            for workspace_folder in workspace_folders {
                let paths_to_search_first = vec![
                    // Possible this is a virtual env
                    workspace_folder.clone(),
                    // Optimize for finding these first.
                    workspace_folder.join(".venv"),
                    workspace_folder.join(".conda"),
                    workspace_folder.join(".virtualenv"),
                    workspace_folder.join("venv"),
                ];
                find_python_environments_in_paths_with_locators(
                    paths_to_search_first.clone(),
                    locators,
                    reporter,
                    true,
                    &[],
                    Some(workspace_folder.clone()),
                );

                // If this is a virtual env folder, no need to scan this.
                if is_venv_dir(&workspace_folder) {
                    continue;
                }

                if let Ok(reader) = fs::read_dir(&workspace_folder) {
                    for folder in reader
                        .filter_map(Result::ok)
                        .filter(|d| d.file_type().is_ok_and(|f| f.is_dir()))
                        .map(|p| p.path())
                        .filter(should_search_for_environments_in_path)
                        .filter(|p| !paths_to_search_first.contains(p))
                    {
                        find_python_environments(
                            vec![folder],
                            reporter,
                            locators,
                            true,
                            &[],
                            Some(workspace_folder.clone()),
                        );
                    }
                }
            }
        });
    });
}

fn find_python_environments(
    paths: Vec<PathBuf>,
    reporter: &dyn Reporter,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    is_workspace_folder: bool,
    global_env_search_paths: &[PathBuf],
    search_path: Option<PathBuf>,
) {
    if paths.is_empty() {
        return;
    }
    thread::scope(|s| {
        for item in paths {
            let locators = locators.clone();
            let search_path = search_path.clone();
            s.spawn(move || {
                find_python_environments_in_paths_with_locators(
                    vec![item],
                    &locators,
                    reporter,
                    is_workspace_folder,
                    global_env_search_paths,
                    search_path,
                );
            });
        }
    });
}

fn find_python_environments_in_paths_with_locators(
    paths: Vec<PathBuf>,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    reporter: &dyn Reporter,
    is_workspace_folder: bool,
    global_env_search_paths: &[PathBuf],
    search_path: Option<PathBuf>,
) {
    for path in paths {
        let executables = if is_workspace_folder {
            // If we're in a workspace folder, then we only need to look for bin/python or bin/python.exe
            // As workspace folders generally have either virtual env or conda env or the like.
            // They will not have environments that will ONLY have a file like `bin/python3`.
            // I.e. bin/python will almost always exist.

            // Paths like /Library/Frameworks/Python.framework/Versions/3.10/bin can end up in the current PATH variable.
            // Hence do not just look for files in a bin directory of the path.
            if let Some(executable) = find_executable(&path) {
                vec![executable]
            } else {
                vec![]
            }
        } else {
            // Paths like /Library/Frameworks/Python.framework/Versions/3.10/bin can end up in the current PATH variable.
            // Hence do not just look for files in a bin directory of the path.
            find_executables(path)
                .into_iter()
                .filter(|p| {
                    // Exclude python2 on macOS
                    if std::env::consts::OS == "macos" {
                        return p.to_str().unwrap_or_default() != "/usr/bin/python2";
                    }
                    true
                })
                .collect::<Vec<PathBuf>>()
        };

        identify_python_executables_using_locators(
            executables,
            locators,
            reporter,
            global_env_search_paths,
            search_path.clone(),
        );
    }
}

fn identify_python_executables_using_locators(
    executables: Vec<PathBuf>,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    reporter: &dyn Reporter,
    global_env_search_paths: &[PathBuf],
    search_path: Option<PathBuf>,
) {
    for exe in executables.into_iter() {
        let executable = exe.clone();
        let env = PythonEnv::new(exe.to_owned(), None, None);
        if let Some(env) = identify_python_environment_using_locators(
            &env,
            locators,
            global_env_search_paths,
            search_path.clone(),
        ) {
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
