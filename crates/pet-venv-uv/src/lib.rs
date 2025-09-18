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

fn is_venv_uv_internal(env: &PythonEnv) -> Option<bool> {
    // Check if there's a pyvenv.cfg file with uv entry
    if let Some(cfg) = PyVenvCfg::find(env.executable.parent()?) {
        return Some(cfg.is_uv());
    }
    if let Some(cfg) = PyVenvCfg::find(&env.prefix.clone()?) {
        return Some(cfg.is_uv());
    }
    Some(false)
}

pub fn is_venv_uv(env: &PythonEnv) -> bool {
    is_venv_uv_internal(env).unwrap_or_default()
}

pub fn is_venv_uv_dir(path: &Path) -> bool {
    if let Some(cfg) = PyVenvCfg::find(path) {
        cfg.is_uv()
    } else {
        false
    }
}

pub struct VenvUv {}

impl VenvUv {
    pub fn new() -> VenvUv {
        VenvUv {}
    }
}

impl Default for VenvUv {
    fn default() -> Self {
        Self::new()
    }
}

impl Locator for VenvUv {
    fn get_kind(&self) -> LocatorKind {
        LocatorKind::VenvUv
    }

    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::VenvUv]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if is_venv_uv(env) {
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
            let name = cfg.and_then(|cfg| cfg.prompt);

            Some(
                PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::VenvUv))
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
        // There are no common global locations for uv virtual environments.
        // We expect the user of this class to call `is_compatible`
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_is_venv_uv_dir_detects_uv_environment() {
        // This test checks if we can detect a UV environment from pyvenv.cfg
        let uv_env_path = PathBuf::from("/tmp/test_uv_env");
        assert!(is_venv_uv_dir(&uv_env_path), "Should detect UV environment");
    }

    #[test]
    fn test_is_venv_uv_dir_does_not_detect_regular_environment() {
        // This test checks if we can properly ignore regular venv environments
        let regular_env_path = PathBuf::from("/tmp/test_regular_env");
        assert!(!is_venv_uv_dir(&regular_env_path), "Should not detect regular venv as UV environment");
    }

    #[test]
    fn test_is_venv_uv_dir_handles_nonexistent_environment() {
        // This test checks if we handle non-existent environments gracefully
        let nonexistent_path = PathBuf::from("/tmp/nonexistent_env");
        assert!(!is_venv_uv_dir(&nonexistent_path), "Should not detect non-existent environment as UV");
    }
}