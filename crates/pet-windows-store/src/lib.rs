// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

mod env_variables;
mod environment_locations;
mod environments;

use crate::env_variables::EnvVariables;
#[cfg(windows)]
use environments::list_store_pythons;
use pet_core::env::PythonEnv;
use pet_core::python_environment::{PythonEnvironment, PythonEnvironmentKind};
use pet_core::reporter::Reporter;
use pet_core::{os_environment::Environment, Locator};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub fn is_windows_app_folder_in_program_files(path: &Path) -> bool {
    path.to_str().unwrap_or_default().to_string().to_lowercase()[1..]
        .starts_with(":\\program files\\windowsapps")
}

pub struct WindowsStore {
    pub env_vars: EnvVariables,
    #[allow(dead_code)]
    environments: Arc<Mutex<Option<Vec<PythonEnvironment>>>>,
}

impl WindowsStore {
    pub fn from(environment: &dyn Environment) -> WindowsStore {
        WindowsStore {
            env_vars: EnvVariables::from(environment),
            environments: Arc::new(Mutex::new(None)),
        }
    }
    #[cfg(windows)]
    fn find_with_cache(&self) -> Option<Vec<PythonEnvironment>> {
        let mut environments = self.environments.lock().unwrap();
        if let Some(environments) = environments.clone() {
            return Some(environments);
        }

        let envs = list_store_pythons(&self.env_vars).unwrap_or_default();
        environments.replace(envs.clone());
        Some(envs)
    }
    #[cfg(windows)]
    fn clear(&self) {
        self.environments.lock().unwrap().take();
    }
}

impl Locator for WindowsStore {
    fn get_name(&self) -> &'static str {
        "WindowsStore" // Do not change this name, as this is used in telemetry.
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::WindowsStore]
    }

    #[cfg(windows)]
    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        use pet_virtualenv::is_virtualenv;

        // Assume we create a virtual env from a python install,
        // Then the exe in the virtual env bin will be a symlink to the homebrew python install.
        // Hence the first part of the condition will be true, but the second part will be false.
        if is_virtualenv(env) {
            return None;
        }
        if let Some(environments) = self.find_with_cache() {
            for found_env in environments {
                if let Some(ref python_executable_path) = found_env.executable {
                    if python_executable_path == &env.executable {
                        return Some(found_env);
                    }
                }
            }
        }
        None
    }

    #[cfg(unix)]
    fn try_from(&self, _env: &PythonEnv) -> Option<PythonEnvironment> {
        None
    }

    #[cfg(windows)]
    fn find(&self, reporter: &dyn Reporter) {
        self.clear();
        if let Some(environments) = self.find_with_cache() {
            environments
                .iter()
                .for_each(|e| reporter.report_environment(e))
        }
    }

    #[cfg(unix)]
    fn find(&self, _reporter: &dyn Reporter) {
        //
    }
}
