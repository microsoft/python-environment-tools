// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

#[cfg(windows)]
use environments::get_registry_pythons;
use pet_conda::{utils::is_conda_env, CondaLocator};
#[cfg(windows)]
use pet_core::LocatorResult;
use pet_core::{
    python_environment::{PythonEnvironment, PythonEnvironmentKind},
    reporter::Reporter,
    Locator,
};
use pet_python_utils::env::PythonEnv;
use pet_virtualenv::is_virtualenv;
use std::sync::{atomic::AtomicBool, Arc, RwLock};

mod environments;

pub struct WindowsRegistry {
    #[allow(dead_code)]
    conda_locator: Arc<dyn CondaLocator>,
    #[allow(dead_code)]
    searched: AtomicBool,
    #[allow(dead_code)]
    environments: Arc<RwLock<Vec<PythonEnvironment>>>,
}

impl WindowsRegistry {
    pub fn from(conda_locator: Arc<dyn CondaLocator>) -> WindowsRegistry {
        WindowsRegistry {
            conda_locator,
            searched: AtomicBool::new(false),
            environments: Arc::new(RwLock::new(vec![])),
        }
    }
    #[cfg(windows)]
    fn find_with_cache(&self) -> Option<LocatorResult> {
        use std::sync::atomic::Ordering;

        if self.searched.load(Ordering::Relaxed) {
            if let Ok(envs) = self.environments.read() {
                return Some(LocatorResult {
                    environments: envs.clone(),
                    managers: vec![],
                });
            }
        }
        self.searched.store(false, Ordering::Relaxed);
        if let Ok(mut envs) = self.environments.write() {
            envs.clear();
        }
        let result = get_registry_pythons(&self.conda_locator)?;
        if let Ok(mut envs) = self.environments.write() {
            envs.clear();
            envs.extend(result.environments.clone());
            self.searched.store(true, Ordering::Relaxed);
        }

        Some(result)
    }
}

impl Locator for WindowsRegistry {
    fn get_name(&self) -> &'static str {
        "WindowsRegistry"
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
        if let Some(result) = self.find_with_cache() {
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
        self.searched
            .store(false, std::sync::atomic::Ordering::Relaxed);
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
