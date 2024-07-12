// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

mod env_variables;
mod environment_locations;
mod environments;

use crate::env_variables::EnvVariables;
#[cfg(windows)]
use environments::list_store_pythons;
use pet_core::python_environment::{PythonEnvironment, PythonEnvironmentKind};
use pet_core::reporter::Reporter;
use pet_core::{os_environment::Environment, Locator};
use pet_python_utils::env::PythonEnv;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

pub fn is_windows_app_folder_in_program_files(path: &Path) -> bool {
    path.to_str().unwrap_or_default().to_string().to_lowercase()[1..]
        .starts_with(":\\program files\\windowsapps")
}

pub struct WindowsStore {
    pub env_vars: EnvVariables,
    #[allow(dead_code)]
    searched: AtomicBool,
    #[allow(dead_code)]
    environments: Arc<RwLock<Vec<PythonEnvironment>>>,
}

impl WindowsStore {
    pub fn from(environment: &dyn Environment) -> WindowsStore {
        WindowsStore {
            searched: AtomicBool::new(false),
            env_vars: EnvVariables::from(environment),
            environments: Arc::new(RwLock::new(vec![])),
        }
    }
    #[cfg(windows)]
    fn find_with_cache(&self) -> Option<Vec<PythonEnvironment>> {
        use std::sync::atomic::Ordering;

        if self.searched.load(Ordering::Relaxed) {
            if let Ok(envs) = self.environments.read() {
                return Some(envs.clone());
            }
        }
        self.searched.store(false, Ordering::Relaxed);
        if let Ok(mut envs) = self.environments.write() {
            envs.clear();
        }
        let environments = list_store_pythons(&self.env_vars)?;
        if let Ok(mut envs) = self.environments.write() {
            envs.clear();
            envs.extend(environments.clone());
            self.searched.store(true, Ordering::Relaxed);
        }
        Some(environments)
    }
    #[cfg(windows)]
    fn clear(&self) {
        use std::sync::atomic::Ordering;

        self.searched.store(false, Ordering::Relaxed);
        if let Ok(mut envs) = self.environments.write() {
            envs.clear();
        }
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
        self.searched
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    #[cfg(unix)]
    fn find(&self, _reporter: &dyn Reporter) {
        //
    }
}
