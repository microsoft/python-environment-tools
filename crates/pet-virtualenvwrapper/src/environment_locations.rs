// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::env_variables::EnvVariables;
use pet_fs::path::norm_case;
use std::{fs, path::PathBuf};

#[cfg(windows)]
fn get_default_virtualenvwrapper_path(env_vars: &EnvVariables) -> Option<PathBuf> {
    // In Windows, the default path for WORKON_HOME is %USERPROFILE%\Envs.
    // If 'Envs' is not available we should default to '.virtualenvs'. Since that
    // is also valid for windows.

    if let Some(user_home) = &env_vars.home {
        let home = user_home.join("Envs");
        if fs::metadata(&home).is_ok() {
            return Some(norm_case(home));
        }
        let home = user_home.join("virtualenvs");
        if fs::metadata(&home).is_ok() {
            return Some(norm_case(home));
        }
    }
    None
}

#[cfg(unix)]
fn get_default_virtualenvwrapper_path(env_vars: &EnvVariables) -> Option<PathBuf> {
    use std::fs;

    if let Some(home) = &env_vars.home {
        let home = home.join(".virtualenvs");
        if fs::metadata(&home).is_ok() {
            return Some(norm_case(&home));
        }
    }
    None
}

pub fn get_work_on_home_path(environment: &EnvVariables) -> Option<PathBuf> {
    // The WORKON_HOME variable contains the path to the root directory of all virtualenvwrapper environments.
    // If the interpreter path belongs to one of them then it is a virtualenvwrapper type of environment.
    if let Some(work_on_home) = &environment.workon_home {
        // TODO: Why do we need to canonicalize the path?
        if let Ok(work_on_home) = std::fs::canonicalize(work_on_home) {
            if fs::metadata(&work_on_home).is_ok() {
                return Some(norm_case(&work_on_home));
            }
        }
    }
    get_default_virtualenvwrapper_path(environment)
}
