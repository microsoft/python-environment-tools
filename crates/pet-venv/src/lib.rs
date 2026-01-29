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

            // Get the name from the prefix if it exists.
            let cfg = PyVenvCfg::find(env.executable.parent()?)
                .or_else(|| PyVenvCfg::find(&env.prefix.clone()?));
            let name = cfg.and_then(|cfg| cfg.prompt);

            Some(
                PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::Venv))
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
    use std::fs;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_is_venv_dir_with_pyvenv_cfg() {
        let dir = tempdir().unwrap();
        let cfg_path = dir.path().join("pyvenv.cfg");
        let mut file = fs::File::create(&cfg_path).unwrap();
        writeln!(file, "version = 3.11.4").unwrap();

        assert!(is_venv_dir(dir.path()));
    }

    #[test]
    fn test_is_venv_dir_without_pyvenv_cfg() {
        let dir = tempdir().unwrap();
        assert!(!is_venv_dir(dir.path()));
    }

    #[test]
    fn test_is_venv_with_pyvenv_cfg_in_parent() {
        let dir = tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let cfg_path = dir.path().join("pyvenv.cfg");
        let mut file = fs::File::create(&cfg_path).unwrap();
        writeln!(file, "version = 3.11.4").unwrap();

        // Create a fake python executable
        let python_path = bin_dir.join("python");
        fs::File::create(&python_path).unwrap();

        let env = PythonEnv::new(python_path, Some(dir.path().to_path_buf()), None);
        assert!(is_venv(&env));
    }

    #[test]
    fn test_is_venv_without_pyvenv_cfg() {
        let dir = tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let python_path = bin_dir.join("python");
        fs::File::create(&python_path).unwrap();

        let env = PythonEnv::new(python_path, Some(dir.path().to_path_buf()), None);
        assert!(!is_venv(&env));
    }

    #[test]
    fn test_venv_locator_kind() {
        let venv = Venv::new();
        assert_eq!(venv.get_kind(), LocatorKind::Venv);
    }

    #[test]
    fn test_venv_supported_categories() {
        let venv = Venv::new();
        let categories = venv.supported_categories();
        assert_eq!(categories.len(), 1);
        assert_eq!(categories[0], PythonEnvironmentKind::Venv);
    }

    #[test]
    fn test_venv_default() {
        let venv = Venv::default();
        assert_eq!(venv.get_kind(), LocatorKind::Venv);
    }

    #[test]
    fn test_venv_try_from_valid_venv() {
        let dir = tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let cfg_path = dir.path().join("pyvenv.cfg");
        let mut file = fs::File::create(&cfg_path).unwrap();
        writeln!(file, "version = 3.11.4").unwrap();
        writeln!(file, "prompt = my-test-env").unwrap();

        let python_path = bin_dir.join("python");
        fs::File::create(&python_path).unwrap();

        let env = PythonEnv::new(python_path.clone(), Some(dir.path().to_path_buf()), None);
        let venv = Venv::new();
        let result = venv.try_from(&env);

        assert!(result.is_some());
        let py_env = result.unwrap();
        assert_eq!(py_env.kind, Some(PythonEnvironmentKind::Venv));
        assert_eq!(py_env.name, Some("my-test-env".to_string()));
        assert_eq!(py_env.executable, Some(python_path));
    }

    #[test]
    fn test_venv_try_from_non_venv() {
        let dir = tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let python_path = bin_dir.join("python");
        fs::File::create(&python_path).unwrap();

        let env = PythonEnv::new(python_path, Some(dir.path().to_path_buf()), None);
        let venv = Venv::new();
        let result = venv.try_from(&env);

        assert!(result.is_none());
    }
}
