// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{trace, warn};
use pet_conda::utils::is_conda_env;
use pet_core::env::PythonEnv;
use pet_core::os_environment::Environment;
use pet_core::python_environment::PythonEnvironmentKind;
use pet_core::reporter::Reporter;
use pet_core::{Configuration, Locator, LocatorKind};
use pet_env_var_path::get_search_paths_from_env_variables;
use pet_global_virtualenvs::list_global_virtual_envs_paths;
use pet_pixi::is_pixi_env;
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
    pub total: Duration,
    pub locators: BTreeMap<LocatorKind, Duration>,
    pub breakdown: BTreeMap<&'static str, Duration>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchScope {
    /// Search for environments in global space.
    Global(PythonEnvironmentKind),
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
        total: Duration::from_secs(0),
        locators: BTreeMap::new(),
        breakdown: BTreeMap::new(),
    }));
    let start = std::time::Instant::now();

    // From settings
    let environment_directories = configuration.environment_directories.unwrap_or_default();
    let workspace_directories = configuration.workspace_directories.unwrap_or_default();
    let executables = configuration.executables.unwrap_or_default();
    let search_global = match search_scope {
        Some(SearchScope::Global(_)) => true,
        Some(SearchScope::Workspace) => false,
        _ => true,
    };
    let search_kind = match search_scope {
        Some(SearchScope::Global(kind)) => Some(kind),
        _ => None,
    };

    thread::scope(|s| {
        // 1. Find using known global locators.
        s.spawn(|| {
            // Find in all the finders
            let start = std::time::Instant::now();
            if search_global {
                thread::scope(|s| {
                    for locator in locators.iter() {
                        if let Some(kind) = &search_kind {
                            if !locator.supported_categories().contains(kind) {
                                trace!(
                                    "Skipping locator: {:?} as it does not support {:?} (required by refresh command)",
                                    locator.get_kind(),
                                    kind
                                );
                                continue;
                            }
                        }

                        let locator = locator.clone();
                        let summary = summary.clone();
                        s.spawn(move || {
                            let start = std::time::Instant::now();
                            trace!("Searching using locator: {:?}", locator.get_kind());
                            locator.find(reporter);
                            trace!(
                                "Completed searching using locator: {:?} in {:?}",
                                locator.get_kind(),
                                start.elapsed()
                            );
                            summary
                                .lock()
                                .unwrap()
                                .locators
                                .insert(locator.get_kind(), start.elapsed());
                        });
                    }
                });
            }
            summary
                .lock()
                .unwrap()
                .breakdown
                .insert("Locators", start.elapsed());
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
            summary
                .lock()
                .unwrap()
                .breakdown
                .insert("Path", start.elapsed());
        });
        // Step 3: Search in some global locations for virtual envs.
        let environment_directories_search = environment_directories.clone();
        s.spawn(|| {
            let start = std::time::Instant::now();
            if search_global {
                let mut possible_environments = vec![];

                // These are directories that contain environments, hence enumerate these directories.
                for directory in environment_directories_search {
                    if let Ok(reader) = fs::read_dir(directory) {
                        possible_environments.append(
                            &mut reader
                                .filter_map(Result::ok)
                                .filter(|d| d.path().is_dir())
                                .map(|p| p.path())
                                .collect(),
                        );
                    }
                }

                let search_paths: Vec<PathBuf> = [
                    list_global_virtual_envs_paths(
                        environment.get_env_var("VIRTUAL_ENV".into()),
                        environment.get_env_var("WORKON_HOME".into()),
                        environment.get_env_var("XDG_DATA_HOME".into()),
                        environment.get_user_home(),
                    ),
                    possible_environments,
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
            summary
                .lock()
                .unwrap()
                .breakdown
                .insert("GlobalVirtualEnvs", start.elapsed());
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
            thread::scope(|s| {
                // Find environments in the workspace folders.
                if !workspace_directories.is_empty() {
                    trace!(
                        "Searching for environments in workspace folders: {:?}",
                        workspace_directories
                    );
                    let global_env_search_paths: Vec<PathBuf> =
                        get_search_paths_from_env_variables(environment);
                    for workspace_folder in workspace_directories {
                        let global_env_search_paths = global_env_search_paths.clone();
                        let environment_directories = environment_directories.clone();
                        s.spawn(move || {
                            find_python_environments_in_workspace_folder_recursive(
                                &workspace_folder,
                                reporter,
                                locators,
                                &global_env_search_paths,
                                &environment_directories,
                            );
                        });
                    }
                }
                // Find the python exes provided.
                if !executables.is_empty() {
                    trace!("Searching for environment executables: {:?}", executables);
                    let global_env_search_paths: Vec<PathBuf> =
                        get_search_paths_from_env_variables(environment);
                    identify_python_executables_using_locators(
                        executables,
                        locators,
                        reporter,
                        &global_env_search_paths,
                    );
                }
            });

            summary
                .lock()
                .unwrap()
                .breakdown
                .insert("Workspaces", start.elapsed());
        });
    });
    summary.lock().unwrap().total = start.elapsed();

    summary
}

pub fn find_python_environments_in_workspace_folder_recursive(
    workspace_folder: &PathBuf,
    reporter: &dyn Reporter,
    locators: &Arc<Vec<Arc<dyn Locator>>>,
    global_env_search_paths: &[PathBuf],
    environment_directories: &[PathBuf],
) {
    // When searching in a directory, give preference to some paths.
    let mut paths_to_search_first = vec![
        // Possible this is a virtual env
        workspace_folder.to_path_buf(),
        // Optimize for finding these first.
        workspace_folder.join(".venv"),
        workspace_folder.join(".conda"),
        workspace_folder.join(".virtualenv"),
        workspace_folder.join("venv"),
    ];

    // Add all subdirectories of .pixi/envs/**
    if let Ok(reader) = fs::read_dir(workspace_folder.join(".pixi").join("envs")) {
        reader
            .filter_map(Result::ok)
            .filter(|d| d.path().is_dir())
            .map(|p| p.path())
            .for_each(|p| paths_to_search_first.push(p));
    }

    // Possible this is an environment.
    find_python_environments_in_paths_with_locators(
        paths_to_search_first.clone(),
        locators,
        reporter,
        true,
        global_env_search_paths,
    );

    // If this is a virtual env folder, no need to scan this.
    // Note: calling is_pixi_env after is_conda_env is redundant but kept for consistency.
    if is_virtualenv_dir(workspace_folder)
        || is_conda_env(workspace_folder)
        || is_pixi_env(workspace_folder)
    {
        return;
    }
    if let Ok(reader) = fs::read_dir(workspace_folder) {
        for folder in reader
            .filter_map(Result::ok)
            .filter(|d| d.path().is_dir())
            .map(|p| p.path())
            .filter(|p| {
                // If this directory is a sub directory or is in the environment_directories, then do not search in this directory.
                if environment_directories.contains(p) {
                    return true;
                }
                if environment_directories.iter().any(|d| p.starts_with(d)) {
                    return true;
                }
                should_search_for_environments_in_path(p)
            })
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
