// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

mod env_variables;
mod environment_locations;
mod environments;

use crate::env_variables::EnvVariables;
#[cfg(windows)]
use environments::list_store_pythons;
use pet_core::python_environment::PythonEnvironment;
use pet_core::reporter::Reporter;
use pet_core::{os_environment::Environment, Locator};
use pet_utils::env::PythonEnv;
use std::path::Path;

pub fn is_windows_app_folder_in_program_files(path: &Path) -> bool {
    path.to_str().unwrap_or_default().to_string().to_lowercase()[1..]
        .starts_with(":\\program files\\windowsapps")
}

pub struct WindowsStore {
    pub env_vars: EnvVariables,
}

impl WindowsStore {
    pub fn from(environment: &dyn Environment) -> WindowsStore {
        WindowsStore {
            env_vars: EnvVariables::from(environment),
        }
    }
}

impl Locator for WindowsStore {
    #[allow(unused_variables)]
    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        #[cfg(windows)]
        let environments = list_store_pythons(&self.env_vars)?;
        #[cfg(windows)]
        for found_env in environments {
            if let Some(ref python_executable_path) = found_env.executable {
                if python_executable_path == &env.executable {
                    return Some(found_env);
                }
            }
        }
        None
    }

    #[cfg(windows)]
    fn find(&self, reporter: &dyn Reporter) {
        if let Some(items) = list_store_pythons(&self.env_vars) {
            items.iter().for_each(|e| reporter.report_environment(e))
        }
    }

    #[cfg(unix)]
    fn find(&self, _reporter: &dyn Reporter) {
        //
    }
}
