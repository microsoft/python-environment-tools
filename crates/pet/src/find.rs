// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{trace, warn};
use pet_conda::utils::is_conda_env;
use pet_core::os_environment::Environment;
use pet_core::reporter::Reporter;
use pet_core::{Configuration, Locator};
use pet_env_var_path::get_search_paths_from_env_variables;
use pet_global_virtualenvs::list_global_virtual_envs_paths;
use pet_python_utils::env::PythonEnv;
use pet_python_utils::executable::{
    find_executable, find_executables, should_search_for_environments_in_path,
};
use pet_virtualenv::is_virtualenv_dir;
use serde::{Deserialize, Serialize};
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
    pub find_workspace_directories_time: Duration,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchScope {
    /// Search for environments in global space.
    Global,
    /// Search for environments in workspace folder.
    Workspace,
}

pub fn find_and_report_envs(
    reporter: &dyn Reporter,
    configuration: Configuration,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    environment: &dyn Environment,
    search_scope: Option<SearchScope>,
) -> Arc<Mutex<Summary>> {
    let summary = Arc::new(Mutex::new(Summary {
        time: Duration::from_secs(0),
        find_locators_times: BTreeMap::new(),
        find_locators_time: Duration::from_secs(0),
        find_path_time: Duration::from_secs(0),
        find_global_virtual_envs_time: Duration::from_secs(0),
        find_workspace_directories_time: Duration::from_secs(0),
    }));
    let start = std::time::Instant::now();

    // From settings
    let environment_directories = configuration.environment_directories.unwrap_or_default();
    let workspace_directories = configuration.workspace_directories.unwrap_or_default();
    let search_global = match search_scope {
        Some(SearchScope::Global) => true,
        Some(SearchScope::Workspace) => false,
        _ => true,
    };
    let search_workspace = match search_scope {
        Some(SearchScope::Global) => false,
        Some(SearchScope::Workspace) => true,
        _ => true,
    };

    thread::scope(|s| {
        // 1. Find using known global locators.
        s.spawn(|| {
            // Find in all the finders
            let start = std::time::Instant::now();
            if search_global {
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
            }
            summary.lock().unwrap().find_locators_time = start.elapsed();
        });
        // Step 2: Search in PATH variable
        s.spawn(|| {
            let start = std::time::Instant::now();
            if search_global {
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
                );
            }
            summary.lock().unwrap().find_path_time = start.elapsed();
        });
        // Step 3: Search in some global locations for virtual envs.
        s.spawn(|| {
            let start = std::time::Instant::now();
            if search_global {
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
                );
            }
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
            let start = std::time::Instant::now();
            if search_workspace && !workspace_directories.is_empty() {
                trace!(
                    "Searching for environments in workspace folders: {:?}",
                    workspace_directories
                );
                let global_env_search_paths: Vec<PathBuf> =
                    get_search_paths_from_env_variables(environment);
                for workspace_folder in workspace_directories {
                    let global_env_search_paths = global_env_search_paths.clone();
                    s.spawn(move || {
                        find_python_environments_in_workspace_folder_recursive(
                            &workspace_folder,
                            reporter,
                            locators,
                            &global_env_search_paths,
                        );
                    });
                }
            }
            summary.lock().unwrap().find_workspace_directories_time = start.elapsed();
        });
    });
    summary.lock().unwrap().time = start.elapsed();

    summary
}

pub fn find_python_environments_in_workspace_folder_recursive(
    workspace_folder: &PathBuf,
    reporter: &dyn Reporter,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    global_env_search_paths: &[PathBuf],
) {
    // When searching in a directory, give preference to some paths.
    let paths_to_search_first = vec![
        // Possible this is a virtual env
        workspace_folder.to_path_buf(),
        // Optimize for finding these first.
        workspace_folder.join(".venv"),
        workspace_folder.join(".conda"),
        workspace_folder.join(".virtualenv"),
        workspace_folder.join("venv"),
    ];

    // Possible this is an environment.
    find_python_environments_in_paths_with_locators(
        paths_to_search_first.clone(),
        locators,
        reporter,
        true,
        global_env_search_paths,
    );

    // If this is a virtual env folder, no need to scan this.
    if is_virtualenv_dir(workspace_folder) || is_conda_env(workspace_folder) {
        return;
    }
    if let Ok(reader) = fs::read_dir(workspace_folder) {
        for folder in reader
            .filter_map(Result::ok)
            .filter(|d| d.file_type().is_ok_and(|f| f.is_dir()))
            .map(|p| p.path())
            .filter(should_search_for_environments_in_path)
            .filter(|p| !paths_to_search_first.contains(p))
        {
            find_python_environments(vec![folder], reporter, locators, true, &[]);
        }
    }
}

fn find_python_environments(
    paths: Vec<PathBuf>,
    reporter: &dyn Reporter,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    is_workspace_folder: bool,
    global_env_search_paths: &[PathBuf],
) {
    if paths.is_empty() {
        return;
    }
    thread::scope(|s| {
        for item in paths {
            let locators = locators.clone();
            s.spawn(move || {
                find_python_environments_in_paths_with_locators(
                    vec![item],
                    &locators,
                    reporter,
                    is_workspace_folder,
                    global_env_search_paths,
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
        );
    }
}

pub fn identify_python_executables_using_locators(
    executables: Vec<PathBuf>,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    reporter: &dyn Reporter,
    global_env_search_paths: &[PathBuf],
) {
    for exe in executables.into_iter() {
        let executable = exe.clone();
        let env = PythonEnv::new(exe.to_owned(), None, None);
        if let Some(env) =
            identify_python_environment_using_locators(&env, locators, global_env_search_paths)
        {
            if let Some(manager) = &env.manager {
                reporter.report_manager(manager);
            }
            reporter.report_environment(&env);
            continue;
        } else {
            warn!("Unknown Python Env {:?}", executable);
        }
    }
}
