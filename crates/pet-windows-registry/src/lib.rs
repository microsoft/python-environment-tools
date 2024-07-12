// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

#[cfg(windows)]
use environments::get_registry_pythons;
use pet_conda::{utils::is_conda_env, CondaLocator};
use pet_core::{
    python_environment::{PythonEnvironment, PythonEnvironmentKind},
    reporter::Reporter,
    Locator, LocatorResult,
};
use pet_python_utils::env::PythonEnv;
use pet_virtualenv::is_virtualenv;
use std::sync::{Arc, Mutex};

mod environments;

pub struct WindowsRegistry {
    #[allow(dead_code)]
    conda_locator: Arc<dyn CondaLocator>,
    #[allow(dead_code)]
    search_result: Arc<Mutex<Option<LocatorResult>>>,
}

impl WindowsRegistry {
    pub fn from(conda_locator: Arc<dyn CondaLocator>) -> WindowsRegistry {
        WindowsRegistry {
            conda_locator,
            search_result: Arc::new(Mutex::new(None)),
        }
    }
    #[cfg(windows)]
    fn find_with_cache(&self, reporter: Option<&dyn Reporter>) -> Option<LocatorResult> {
        let mut result = self.search_result.lock().unwrap();
        if let Some(result) = result.clone() {
            return Some(result);
        }

        let registry_result = get_registry_pythons(&self.conda_locator, &reporter);
        result.replace(registry_result.clone());

        Some(registry_result)
    }
    #[cfg(windows)]
    fn clear(&self) {
        let mut search_result = self.search_result.lock().unwrap();
        search_result.take();
    }
}

impl Locator for WindowsRegistry {
    fn get_name(&self) -> &'static str {
        "WindowsRegistry" // Do not change this name, as this is used in telemetry.
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::WindowsRegistry]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        // Assume we create a virtual env from a python install,
        // Then the exe in the virtual env bin will be a symlink to the homebrew python install.
        // Hence the first part of the condition will be true, but the second part will be false.
        if is_virtualenv(env) {
            return None;
        }
        // We need to check this here, as its possible to install
        // a Python environment via an Installer that ends up in Windows Registry
        // However that environment is a conda environment.
        if let Some(env_path) = &env.prefix {
            if is_conda_env(env_path) {
                return None;
            }
        }
        #[cfg(windows)]
        if let Some(result) = self.find_with_cache(None) {
            // Find the same env here
            for found_env in result.environments {
                if let Some(ref python_executable_path) = found_env.executable {
                    if python_executable_path == &env.executable {
                        return Some(found_env);
                    }
                }
            }
        }
        None
    }

    #[cfg(windows)]
    fn find(&self, reporter: &dyn Reporter) {
        self.clear();
        let _ = self.find_with_cache(Some(reporter));
    }
    #[cfg(unix)]
    fn find(&self, _reporter: &dyn Reporter) {
        //
    }
}
