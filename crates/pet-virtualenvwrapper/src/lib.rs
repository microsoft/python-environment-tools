// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use env_variables::EnvVariables;
use environment_locations::get_work_on_home_path;
use environments::{get_project, is_virtualenvwrapper, list_python_environments};
use pet_core::{
    os_environment::Environment,
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentCategory},
    Locator, LocatorResult,
};
use pet_utils::{env::PythonEnv, headers::Headers};

mod env_variables;
mod environment_locations;
mod environments;

pub struct VirtualEnvWrapper {
    pub env_vars: EnvVariables,
}

impl VirtualEnvWrapper {
    pub fn from(environment: &dyn Environment) -> VirtualEnvWrapper {
        VirtualEnvWrapper {
            env_vars: EnvVariables::from(environment),
        }
    }
}

impl Locator for VirtualEnvWrapper {
    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if !is_virtualenvwrapper(env, &self.env_vars) {
            return None;
        }
        let mut name = None;
        if let Some(prefix) = &env.prefix {
            if let Some(filename) = prefix.file_name() {
                name = filename.to_str().map(|f| f.to_string());
            }
        }
        let version = match env.version {
            Some(ref v) => Some(v.clone()),
            None => match &env.prefix {
                Some(prefix) => Headers::get_version(prefix),
                None => None,
            },
        };

        Some(
            PythonEnvironmentBuilder::new(PythonEnvironmentCategory::VirtualEnvWrapper)
                .name(name)
                .executable(Some(env.executable.clone()))
                .version(version)
                .prefix(env.prefix.clone())
                .project(get_project(env))
                .build(),
        )
    }

    fn find(&self) -> Option<LocatorResult> {
        let work_on_home = get_work_on_home_path(&self.env_vars)?;
        let envs = list_python_environments(&work_on_home)?;
        let mut environments: Vec<PythonEnvironment> = vec![];
        envs.iter().for_each(|env| {
            if let Some(env) = self.from(env) {
                environments.push(env);
            }
        });
        if environments.is_empty() {
            None
        } else {
            Some(LocatorResult {
                managers: vec![],
                environments,
            })
        }
    }
}
