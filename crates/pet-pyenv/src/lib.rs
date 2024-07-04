// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use env_variables::EnvVariables;
use environments::{
    get_generic_python_environment, get_virtual_env_environment, list_pyenv_environments,
};
use manager::PyEnvInfo;
use pet_conda::{utils::is_conda_env, CondaLocator};
use pet_core::{
    manager::{EnvManager, EnvManagerType},
    os_environment::Environment,
    python_environment::{PythonEnvironment, PythonEnvironmentKind},
    reporter::Reporter,
    Locator,
};
use pet_python_utils::env::PythonEnv;

pub mod env_variables;
mod environment_locations;
mod environments;
mod manager;

pub struct PyEnv {
    pub env_vars: EnvVariables,
    pub conda_locator: Arc<dyn CondaLocator>,
    manager: Arc<Mutex<Option<EnvManager>>>,
    versions_dir: Arc<Mutex<Option<PathBuf>>>,
}

impl PyEnv {
    pub fn from(
        environment: &dyn Environment,
        conda_locator: Arc<dyn CondaLocator>,
    ) -> impl Locator {
        PyEnv {
            env_vars: EnvVariables::from(environment),
            conda_locator,
            manager: Arc::new(Mutex::new(None)),
            versions_dir: Arc::new(Mutex::new(None)),
        }
    }
}

impl Locator for PyEnv {
    fn get_name(&self) -> &'static str {
        "PyEnv"
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![
            PythonEnvironmentKind::Pyenv,
            PythonEnvironmentKind::PyenvVirtualEnv,
        ]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if let Some(prefix) = &env.prefix {
            if is_conda_env(prefix) {
                return None;
            }
        }
        // Possible this is a root conda env (hence parent directory is conda install dir).
        if is_conda_env(env.executable.parent()?) {
            return None;
        }
        // Possible this is a conda env (hence parent directory is Scripts/bin dir).
        if is_conda_env(env.executable.parent()?.parent()?) {
            return None;
        }

        // Env path must exists,
        // If exe is Scripts/python.exe or bin/python.exe
        // Then env path is parent of Scripts or bin
        // & in pyenv case thats a directory inside `versions` folder.
        let mut binding_manager = self.manager.lock();
        let managers = binding_manager.as_mut().unwrap();
        let mut binding_versions = self.versions_dir.lock();
        let versions = binding_versions.as_mut().unwrap();
        if managers.is_none() || versions.is_none() {
            let pyenv_info = PyEnvInfo::from(&self.env_vars);
            let mut manager: Option<EnvManager> = None;
            if let Some(ref exe) = pyenv_info.exe {
                let version = pyenv_info.version.clone();
                manager = Some(EnvManager::new(exe.clone(), EnvManagerType::Pyenv, version));
            }
            if let Some(version_path) = &pyenv_info.versions {
                versions.replace(version_path.clone());
            } else {
                versions.take();
            }
            if let Some(manager) = manager {
                managers.replace(manager.clone());
            } else {
                managers.take();
            }
        }

        if let Some(versions) = versions.clone() {
            let manager = managers.clone();
            if env.executable.starts_with(versions) {
                let env_path = env.prefix.clone()?;
                if let Some(env) = get_virtual_env_environment(&env.executable, &env_path, &manager)
                {
                    return Some(env);
                } else if let Some(env) =
                    get_generic_python_environment(&env.executable, &env_path, &manager)
                {
                    return Some(env);
                }
            }
        }
        None
    }

    fn find(&self, reporter: &dyn Reporter) {
        let pyenv_info = PyEnvInfo::from(&self.env_vars);
        let mut manager: Option<EnvManager> = None;
        if let Some(ref exe) = pyenv_info.exe {
            let version = pyenv_info.version.clone();
            let mgr = EnvManager::new(exe.clone(), EnvManagerType::Pyenv, version);
            reporter.report_manager(&mgr);
            manager = Some(mgr);
        }
        if let Some(ref versions) = &pyenv_info.versions {
            if let Some(envs) = list_pyenv_environments(&manager, versions, &self.conda_locator) {
                for env in envs.environments {
                    reporter.report_environment(&env);
                }
                for mgr in envs.managers {
                    reporter.report_manager(&mgr);
                }
            }
        }
    }
}
