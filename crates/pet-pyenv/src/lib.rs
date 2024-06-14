// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::sync::Arc;

use env_variables::EnvVariables;
use environments::{
    get_generic_environment, get_pure_python_environment, get_virtual_env_environment,
    list_pyenv_environments,
};
use manager::PyEnvInfo;
use pet_conda::CondaLocator;
use pet_core::{
    manager::{EnvManager, EnvManagerType},
    os_environment::Environment,
    python_environment::PythonEnvironment,
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
}

impl PyEnv {
    pub fn from(
        environment: &dyn Environment,
        conda_locator: Arc<dyn CondaLocator>,
    ) -> impl Locator {
        PyEnv {
            env_vars: EnvVariables::from(environment),
            conda_locator,
        }
    }
}

impl Locator for PyEnv {
    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        // Env path must exists,
        // If exe is Scripts/python.exe or bin/python.exe
        // Then env path is parent of Scripts or bin
        // & in pyenv case thats a directory inside `versions` folder.
        let pyenv_info = PyEnvInfo::from(&self.env_vars);
        let mut manager: Option<EnvManager> = None;
        if let Some(ref exe) = pyenv_info.exe {
            let version = pyenv_info.version.clone();
            manager = Some(EnvManager::new(exe.clone(), EnvManagerType::Pyenv, version));
        }

        let versions = &pyenv_info.versions?;
        if env.executable.starts_with(versions) {
            let env_path = env.prefix.clone()?;
            if let Some(env) = get_pure_python_environment(&env.executable, &env_path, &manager) {
                return Some(env);
            } else if let Some(env) =
                get_virtual_env_environment(&env.executable, &env_path, &manager)
            {
                return Some(env);
            } else if let Some(env) = get_generic_environment(&env.executable, &env_path, &manager)
            {
                return Some(env);
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
