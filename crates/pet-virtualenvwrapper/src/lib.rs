// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use env_variables::EnvVariables;
use environments::{get_project, is_virtualenvwrapper};
use pet_core::{
    os_environment::Environment,
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind},
    reporter::Reporter,
    Locator,
};
use pet_python_utils::version;
use pet_python_utils::{env::PythonEnv, executable::find_executables};

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
    fn get_name(&self) -> &'static str {
        "VirtualEnvWrapper"
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::VirtualEnvWrapper]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if !is_virtualenvwrapper(env, &self.env_vars) {
            return None;
        }
        let version = match env.version {
            Some(ref v) => Some(v.clone()),
            None => match &env.prefix {
                Some(prefix) => version::from_creator_for_virtual_env(prefix),
                None => None,
            },
        };
        let mut symlinks = vec![];
        if let Some(ref prefix) = env.prefix {
            symlinks.append(&mut find_executables(prefix));
        }

        Some(
            PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::VirtualEnvWrapper))
                .executable(Some(env.executable.clone()))
                .version(version)
                .prefix(env.prefix.clone())
                .project(get_project(env))
                .symlinks(Some(symlinks))
                .build(),
        )
    }

    fn find(&self, _reporter: &dyn Reporter) {}
}
