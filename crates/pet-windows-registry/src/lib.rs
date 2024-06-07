// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

#[cfg(windows)]
use environments::get_registry_pythons;
use pet_conda::{utils::is_conda_env, CondaLocator};
use pet_core::{python_environment::PythonEnvironment, reporter::Reporter, Locator};
use pet_utils::env::PythonEnv;
use std::sync::Arc;

mod environments;

pub struct WindowsRegistry {
    #[allow(dead_code)]
    conda_locator: Arc<dyn CondaLocator>,
}

impl WindowsRegistry {
    pub fn from(conda_locator: Arc<dyn CondaLocator>) -> WindowsRegistry {
        WindowsRegistry { conda_locator }
    }
}

impl Locator for WindowsRegistry {
    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if let Some(env_path) = &env.prefix {
            if is_conda_env(env_path) {
                return None;
            }
        }
        #[cfg(windows)]
        if let Some(result) = get_registry_pythons(&self.conda_locator) {
            // Find the same env here
            for found_env in result.environments {
                if env.executable.to_str() == env.executable.to_str() {
                    return Some(found_env);
                }
            }
        }
        None
    }

    #[cfg(windows)]
    fn find(&self, reporter: &dyn Reporter) {
        if let Some(result) = get_registry_pythons(&self.conda_locator) {
            result
                .managers
                .iter()
                .for_each(|m| reporter.report_manager(m));
            result
                .environments
                .iter()
                .for_each(|e| reporter.report_environment(e));
        }
    }
    #[cfg(unix)]
    fn find(&self, _reporter: &dyn Reporter) {
        //
    }
}
