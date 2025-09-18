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

pub fn is_venv_uv(env: &PythonEnv) -> bool {
    if let Some(cfg) = PyVenvCfg::find(env.executable.parent().unwrap_or(Path::new(""))) {
        return cfg.is_uv();
    }
    if let Some(ref prefix) = env.prefix {
        if let Some(cfg) = PyVenvCfg::find(prefix) {
            return cfg.is_uv();
        }
    }
    false
}

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

            // Determine the environment kind based on whether it was created with UV
            let kind = if cfg.as_ref().map_or(false, |c| c.is_uv()) {
                PythonEnvironmentKind::VenvUv
            } else {
                PythonEnvironmentKind::Venv
            };

            Some(
                PythonEnvironmentBuilder::new(Some(kind))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_is_venv_uv_dir_detects_uv_environment() {
        use std::fs;
        let test_dir = PathBuf::from("/tmp/test_uv_env_venv");
        fs::create_dir_all(&test_dir).unwrap();
        let pyvenv_cfg = test_dir.join("pyvenv.cfg");
        let contents = "home = /usr/bin/python3.12\nimplementation = CPython\nuv = 0.8.14\nversion_info = 3.12.11\ninclude-system-site-packages = false\nprompt = test-uv-env\n";
        fs::write(&pyvenv_cfg, contents).unwrap();

        assert!(is_venv_uv_dir(&test_dir), "Should detect UV environment");

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_is_venv_uv_dir_does_not_detect_regular_environment() {
        use std::fs;
        let test_dir = PathBuf::from("/tmp/test_regular_env_venv");
        fs::create_dir_all(&test_dir).unwrap();
        let pyvenv_cfg = test_dir.join("pyvenv.cfg");
        let contents = "home = /usr/bin/python3.12\ninclude-system-site-packages = false\nversion = 3.13.5\nexecutable = /usr/bin/python3.12\ncommand = python -m venv /path/to/env\n";
        fs::write(&pyvenv_cfg, contents).unwrap();

        assert!(
            !is_venv_uv_dir(&test_dir),
            "Should not detect regular venv as UV environment"
        );

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_is_venv_uv_dir_handles_nonexistent_environment() {
        let nonexistent_path = PathBuf::from("/tmp/nonexistent_env");
        assert!(
            !is_venv_uv_dir(&nonexistent_path),
            "Should not detect non-existent environment as UV"
        );
    }

    #[test]
    fn test_venv_locator_detects_uv_kind() {
        use pet_core::env::PythonEnv;
        use std::fs;
        
        // Create a test UV environment
        let test_dir = PathBuf::from("/tmp/test_locator_uv");
        let bin_dir = test_dir.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        
        let pyvenv_cfg = test_dir.join("pyvenv.cfg");
        let contents = "home = /usr/bin/python3.12\nimplementation = CPython\nuv = 0.8.14\nversion_info = 3.12.11\ninclude-system-site-packages = false\nprompt = test-uv-env\n";
        fs::write(&pyvenv_cfg, contents).unwrap();
        
        let python_exe = bin_dir.join("python");
        fs::write(&python_exe, "").unwrap(); // Create dummy python executable
        
        let env = PythonEnv::new(python_exe.clone(), Some(test_dir.clone()), Some("3.12.11".to_string()));
        let locator = Venv::new();
        
        if let Some(result) = locator.try_from(&env) {
            assert_eq!(result.kind, Some(PythonEnvironmentKind::VenvUv), "UV environment should be detected as VenvUv");
        } else {
            panic!("Locator should detect UV environment");
        }
        
        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_venv_locator_detects_regular_venv_kind() {
        use pet_core::env::PythonEnv;
        use std::fs;
        
        // Create a test regular venv environment
        let test_dir = PathBuf::from("/tmp/test_locator_regular");
        let bin_dir = test_dir.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        
        let pyvenv_cfg = test_dir.join("pyvenv.cfg");
        let contents = "home = /usr/bin/python3.12\ninclude-system-site-packages = false\nversion = 3.13.5\nexecutable = /usr/bin/python3.12\ncommand = python -m venv /path/to/env\n";
        fs::write(&pyvenv_cfg, contents).unwrap();
        
        let python_exe = bin_dir.join("python");
        fs::write(&python_exe, "").unwrap(); // Create dummy python executable
        
        let env = PythonEnv::new(python_exe.clone(), Some(test_dir.clone()), Some("3.13.5".to_string()));
        let locator = Venv::new();
        
        if let Some(result) = locator.try_from(&env) {
            assert_eq!(result.kind, Some(PythonEnvironmentKind::Venv), "Regular venv should be detected as Venv");
        } else {
            panic!("Locator should detect regular venv environment");
        }
        
        fs::remove_dir_all(&test_dir).ok();
    }
}
