// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use pet_conda::{utils::is_conda_env, CondaLocator};
use pet_core::{
    arch::Architecture,
    manager::EnvManager,
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentCategory},
    LocatorResult,
};
use pet_utils::{executable::find_executable, pyvenv_cfg::PyVenvCfg};
use regex::Regex;
use std::{fs, path::Path, sync::Arc};

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
    let mut envs: Vec<PythonEnvironment> = vec![];
    let mut managers: Vec<EnvManager> = vec![];

    for path in fs::read_dir(versions_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
    {
        if let Some(executable) = find_executable(&path) {
            if let Some(env) = get_pure_python_environment(&executable, &path, manager) {
                envs.push(env);
            } else if let Some(env) = get_virtual_env_environment(&executable, &path, manager) {
                envs.push(env);
            } else if is_conda_env(&path) {
                if let Some(result) = conda_locator.find_in(&path) {
                    result.environments.iter().for_each(|e| {
                        envs.push(e.clone());
                    });
                    result.managers.iter().for_each(|e| {
                        managers.push(e.clone());
                    });
                }
            }
        }
    }

    Some(LocatorResult {
        managers,
        environments: envs,
    })
}

pub fn get_pure_python_environment(
    executable: &Path,
    path: &Path,
    manager: &Option<EnvManager>,
) -> Option<PythonEnvironment> {
    let file_name = path.file_name()?.to_string_lossy().to_string();
    let version = get_version(&file_name)?;

    let arch = if file_name.ends_with("-win32") {
        Some(Architecture::X86)
    } else {
        None
    };

    Some(
        PythonEnvironmentBuilder::new(PythonEnvironmentCategory::Pyenv)
            .executable(Some(executable.to_path_buf()))
            .version(Some(version))
            .prefix(Some(path.to_path_buf()))
            .manager(manager.clone())
            .arch(arch)
            // .symlinks(Some(vec![executable.to_path_buf()]))
            .build(),
    )
}

pub fn get_virtual_env_environment(
    executable: &Path,
    path: &Path,
    manager: &Option<EnvManager>,
) -> Option<PythonEnvironment> {
    let pyenv_cfg = PyVenvCfg::find(executable.parent()?)?;
    let folder_name = path.file_name().unwrap().to_string_lossy().to_string();
    Some(
        PythonEnvironmentBuilder::new(PythonEnvironmentCategory::PyenvVirtualEnv)
            // .project(Some(folder_name))
            .name(Some(folder_name))
            .executable(Some(executable.to_path_buf()))
            .version(Some(pyenv_cfg.version))
            .prefix(Some(path.to_path_buf()))
            .manager(manager.clone())
            // .symlinks(Some(vec![executable.to_path_buf()]))
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
