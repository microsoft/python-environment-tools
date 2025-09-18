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

/// Check if this is a UV-created virtual environment
pub fn is_venv_uv(env: &PythonEnv) -> bool {
    if let Some(cfg) =
        PyVenvCfg::find(env.executable.parent().unwrap_or(&env.executable)).or_else(|| {
            PyVenvCfg::find(&env.prefix.clone().unwrap_or_else(|| {
                env.executable
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .to_path_buf()
            }))
        })
    {
        cfg.is_uv()
    } else {
        false
    }
}

/// Check if a directory contains a UV-created virtual environment
pub fn is_venv_uv_dir(path: &Path) -> bool {
    if let Some(cfg) = PyVenvCfg::find(path) {
        cfg.is_uv()
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
    fn get_kind(&self) -> LocatorKind {
        LocatorKind::Venv
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::Venv, PythonEnvironmentKind::VenvUv]
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

            // Get the name from the prefix if it exists.
            let cfg = PyVenvCfg::find(env.executable.parent()?)
                .or_else(|| PyVenvCfg::find(&env.prefix.clone()?));
            let name = cfg.as_ref().and_then(|cfg| cfg.prompt.clone());

            // Determine environment kind based on whether UV was used
            let kind = match &cfg {
                Some(cfg) if cfg.is_uv() => Some(PythonEnvironmentKind::VenvUv),
                Some(_) => Some(PythonEnvironmentKind::Venv),
                None => Some(PythonEnvironmentKind::Venv), // Default to Venv if no cfg found
            };

            Some(
                PythonEnvironmentBuilder::new(kind)
                    .name(name)
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
