// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::path::Path;

use pet_core::{
    env::PythonEnv,
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind},
    pyvenv_cfg::PyVenvCfg,
    reporter::Reporter,
    Locator, LocatorKind,
};
use pet_python_utils::executable::find_executables;
use pet_python_utils::version;

fn is_venv_internal(env: &PythonEnv) -> Option<bool> {
    // env path cannot be empty.
    Some(
        PyVenvCfg::find(env.executable.parent()?).is_some()
            || PyVenvCfg::find(&env.prefix.clone()?).is_some(),
    )
}
pub fn is_venv(env: &PythonEnv) -> bool {
    is_venv_internal(env).unwrap_or_default()
}
pub fn is_venv_dir(path: &Path) -> bool {
    PyVenvCfg::find(path).is_some()
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
    fn get_kind(&self) -> LocatorKind {
        LocatorKind::Venv
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::Venv]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if is_venv(env) {
            let mut prefix = env.prefix.clone();
            if prefix.is_none() {
                prefix = Some(env.executable.parent()?.parent()?.to_path_buf());
            }
            let version = match env.version {
                Some(ref v) => Some(v.clone()),
                None => match &prefix {
                    Some(prefix) => version::from_creator_for_virtual_env(prefix),
                    None => None,
                },
            };
            let mut symlinks = vec![];
            if let Some(ref prefix) = prefix {
                symlinks.append(&mut find_executables(prefix));
            }
            Some(
                PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::Venv))
                    .executable(Some(env.executable.clone()))
                    .version(version)
                    .prefix(prefix)
                    .symlinks(Some(symlinks))
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
