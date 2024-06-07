// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_core::{
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentCategory},
    reporter::Reporter,
    Locator,
};
use pet_utils::{env::PythonEnv, headers::Headers, pyvenv_cfg::PyVenvCfg};

fn is_venv_internal(env: &PythonEnv) -> Option<bool> {
    // env path cannot be empty.
    Some(
        PyVenvCfg::find(env.executable.parent()?).is_some()
            || PyVenvCfg::find(&env.prefix.clone()?).is_some(),
    )
}
pub fn is_venv(env: &PythonEnv) -> bool {
    if let Some(result) = is_venv_internal(env) {
        result
    } else {
        false
    }
}
pub struct Venv {}

impl Venv {
    pub fn new() -> Venv {
        Venv {}
    }
}
impl Default for Venv {
    fn default() -> Self {
        Self::new()
    }
}
impl Locator for Venv {
    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if is_venv(env) {
            let mut name = None;
            if let Some(filename) = &env.prefix {
                name = filename.to_str().map(|f| f.to_string());
            }
            let version = match env.version {
                Some(ref v) => Some(v.clone()),
                None => match &env.prefix {
                    Some(prefix) => Headers::get_version(prefix),
                    None => None,
                },
            };
            Some(
                PythonEnvironmentBuilder::new(PythonEnvironmentCategory::Venv)
                    .name(name)
                    .executable(Some(env.executable.clone()))
                    .version(version)
                    .prefix(env.prefix.clone())
                    .build(),
            )
        } else {
            None
        }
    }

    fn find(&self, _reporter: &dyn Reporter) {
        // There are no common global locations for virtual environments.
        // We expect the user of this class to call `is_compatible`
    }
}
