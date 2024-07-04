// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use pet_conda::{utils::is_conda_env, CondaLocator};
use pet_core::{
    arch::Architecture,
    manager::EnvManager,
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind},
    LocatorResult,
};
use pet_python_utils::executable::{find_executable, find_executables};
use pet_python_utils::version;
use regex::Regex;
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
    thread,
};

lazy_static! {
    // Stable Versions = like 3.10.10
    static ref PURE_PYTHON_VERSION: Regex = Regex::new(r"^(\d+\.\d+\.\d+)$")
        .expect("error parsing Version regex for Python Version in pyenv");
    // Dev Versions = like 3.10-dev
    static ref DEV_PYTHON_VERSION: Regex = Regex::new(r"^(\d+\.\d+-.*)$")
        .expect("error parsing Version regex for Dev Python Version in pyenv");
    // Alpha, rc Versions = like 3.10.0a3
    static ref BETA_PYTHON_VERSION: Regex = Regex::new(r"^(\d+\.\d+.\d+\w\d+)")
        .expect("error parsing Version regex for Alpha Python Version in pyenv");
    // win32 versions, rc Versions = like 3.11.0a-win32
    static ref WIN32_PYTHON_VERSION: Regex = Regex::new(r"^(\d+\.\d+.\d+\w\d+)-win32")
        .expect("error parsing Version regex for Win32 Python Version in pyenv");
}

pub fn list_pyenv_environments(
    manager: &Option<EnvManager>,
    versions_dir: &Path,
    conda_locator: &Arc<dyn CondaLocator>,
) -> Option<LocatorResult> {
    let envs = Arc::new(Mutex::new(vec![]));
    let managers = Arc::new(Mutex::new(vec![]));

    thread::scope(|s| {
        if let Ok(reader) = fs::read_dir(versions_dir) {
            for path in reader.filter_map(Result::ok).map(|e| e.path()) {
                if let Some(executable) = find_executable(&path) {
                    let path = path.clone();
                    let executable = executable.clone();
                    let conda_locator = conda_locator.clone();
                    let manager = manager.clone();
                    let envs = envs.clone();
                    let managers = managers.clone();
                    s.spawn(move || {
                        if is_conda_env(&path) {
                            if let Some(result) = conda_locator.find_in(&path) {
                                result.environments.iter().for_each(|e| {
                                    envs.lock().unwrap().push(e.clone());
                                });
                                result.managers.iter().for_each(|e| {
                                    managers.lock().unwrap().push(e.clone());
                                });
                            }
                        } else if let Some(env) =
                            get_virtual_env_environment(&executable, &path, &manager)
                        {
                            envs.lock().unwrap().push(env);
                        } else if let Some(env) =
                            get_generic_python_environment(&executable, &path, &manager)
                        {
                            envs.lock().unwrap().push(env);
                        }
                    });
                }
            }
        }
    });

    let managers = managers.lock().unwrap();
    let envs = envs.lock().unwrap();
    Some(LocatorResult {
        managers: managers.clone(),
        environments: envs.clone(),
    })
}

pub fn get_generic_python_environment(
    executable: &Path,
    path: &Path,
    manager: &Option<EnvManager>,
) -> Option<PythonEnvironment> {
    let file_name = path.file_name()?.to_string_lossy().to_string();
    // If we can get the version from the header files, thats more accurate.
    let version = version::from_header_files(path).or_else(|| get_version(&file_name));

    let arch = if file_name.ends_with("-win32") {
        Some(Architecture::X86)
    } else {
        None
    };

    Some(
        PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::Pyenv))
            .executable(Some(executable.to_path_buf()))
            .version(version)
            .prefix(Some(path.to_path_buf()))
            .manager(manager.clone())
            .arch(arch)
            .symlinks(Some(find_executables(path)))
            .build(),
    )
}

pub fn get_virtual_env_environment(
    executable: &Path,
    path: &Path,
    manager: &Option<EnvManager>,
) -> Option<PythonEnvironment> {
    let version = version::from_pyvenv_cfg(path)?;
    Some(
        PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::PyenvVirtualEnv))
            .executable(Some(executable.to_path_buf()))
            .version(Some(version))
            .prefix(Some(path.to_path_buf()))
            .manager(manager.clone())
            .symlinks(Some(find_executables(path)))
            .build(),
    )
}

fn get_version(folder_name: &str) -> Option<String> {
    // Stable Versions = like 3.10.10
    match PURE_PYTHON_VERSION.captures(folder_name) {
        Some(captures) => captures.get(1).map(|version| version.as_str().to_string()),
        None => {
            // Dev Versions = like 3.10-dev
            match DEV_PYTHON_VERSION.captures(folder_name) {
                Some(captures) => captures.get(1).map(|version| version.as_str().to_string()),
                None => {
                    // Alpha, rc Versions = like 3.10.0a3
                    match BETA_PYTHON_VERSION.captures(folder_name) {
                        Some(captures) => {
                            captures.get(1).map(|version| version.as_str().to_string())
                        }
                        None => {
                            // win32 versions, rc Versions = like 3.11.0a-win32
                            match WIN32_PYTHON_VERSION.captures(folder_name) {
                                Some(captures) => {
                                    captures.get(1).map(|version| version.as_str().to_string())
                                }
                                None => None,
                            }
                        }
                    }
                }
            }
        }
    }
}
