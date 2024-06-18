// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

#[cfg(windows)]
use environments::get_registry_pythons;
use pet_conda::{utils::is_conda_env, CondaLocator};
#[cfg(windows)]
use pet_core::LocatorResult;
use pet_core::{python_environment::PythonEnvironment, reporter::Reporter, Locator};
use pet_python_utils::env::PythonEnv;
use std::sync::{Arc, RwLock};

mod environments;

pub struct WindowsRegistry {
    #[allow(dead_code)]
    conda_locator: Arc<dyn CondaLocator>,
    #[allow(dead_code)]
    environments: Arc<RwLock<Option<Vec<PythonEnvironment>>>>,
}

impl WindowsRegistry {
    pub fn from(conda_locator: Arc<dyn CondaLocator>) -> WindowsRegistry {
        WindowsRegistry {
            conda_locator,
            environments: Arc::new(RwLock::new(None)),
        }
    }
    #[cfg(windows)]
    fn find_with_cache(&self) -> Option<LocatorResult> {
        let envs = self.environments.read().unwrap();
        if let Some(environments) = envs.as_ref() {
            Some(LocatorResult {
                managers: vec![],
                environments: environments.clone(),
            })
        } else {
            drop(envs);
            let mut envs = self.environments.write().unwrap();
            let result = get_registry_pythons(&self.conda_locator)?;
            envs.replace(result.environments.clone());

            Some(result)
        }
    }
}

impl Locator for WindowsRegistry {
    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        // We need to check this here, as its possible to install
        // a Python environment via an Installer that ends up in Windows Registry
        // However that environment is a conda environment.
        if let Some(env_path) = &env.prefix {
            if is_conda_env(env_path) {
                return None;
            }
        }
        #[cfg(windows)]
        if let Some(result) = self.find_with_cache() {
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
        let mut envs = self.environments.write().unwrap();
        if envs.is_some() {
            envs.take();
        }
        if let Some(result) = self.find_with_cache() {
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
