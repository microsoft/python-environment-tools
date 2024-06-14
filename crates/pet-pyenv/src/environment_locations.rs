// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::env_variables::EnvVariables;
use pet_fs::path::norm_case;
use std::{fs, path::PathBuf};

#[cfg(windows)]
pub fn get_home_pyenv_dir(env_vars: &EnvVariables) -> Option<PathBuf> {
    let home = env_vars.home.clone()?;
    Some(norm_case(home.join(".pyenv").join("pyenv-win")))
}

#[cfg(unix)]
pub fn get_home_pyenv_dir(env_vars: &EnvVariables) -> Option<PathBuf> {
    let home = env_vars.home.clone()?;
    Some(norm_case(home.join(".pyenv")))
}

pub fn get_binary_from_known_paths(env_vars: &EnvVariables) -> Option<PathBuf> {
    for known_path in &env_vars.known_global_search_locations {
        let exe = if cfg!(windows) {
            known_path.join("pyenv.exe")
        } else {
            known_path.join("pyenv")
        };
        if let Ok(metadata) = fs::metadata(&exe) {
            if metadata.is_file() {
                return Some(norm_case(exe));
            }
        }
    }
    None
}

pub fn get_pyenv_dir(env_vars: &EnvVariables) -> Option<PathBuf> {
    // Check if the pyenv environment variables exist: PYENV on Windows, PYENV_ROOT on Unix.
    // They contain the path to pyenv's installation folder.
    // If they don't exist, use the default path: ~/.pyenv/pyenv-win on Windows, ~/.pyenv on Unix.
    // If the interpreter path starts with the path to the pyenv folder, then it is a pyenv environment.
    // See https://github.com/pyenv/pyenv#locating-the-python-installation for general usage,
    // And https://github.com/pyenv-win/pyenv-win for Windows specifics.

    match &env_vars.pyenv_root {
        Some(dir) => Some(PathBuf::from(dir)),
        None => env_vars.pyenv.as_ref().map(PathBuf::from),
    }
}
