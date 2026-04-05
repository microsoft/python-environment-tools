// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

use pet_conda::package::{CondaPackageInfo, Package};
use pet_core::{
    env::PythonEnv,
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind},
    reporter::Reporter,
    Locator, LocatorKind,
};
use pet_python_utils::executable::find_executables;

pub fn is_pixi_env(path: &Path) -> bool {
    path.join("conda-meta").join("pixi").is_file()
}

fn get_pixi_prefix(env: &PythonEnv) -> Option<PathBuf> {
    env.prefix.clone().or_else(|| {
        env.executable.parent().and_then(|parent_dir| {
            if is_pixi_env(parent_dir) {
                Some(parent_dir.to_path_buf())
            } else if parent_dir.ends_with("bin") || parent_dir.ends_with("Scripts") {
                parent_dir
                    .parent()
                    .filter(|parent| is_pixi_env(parent))
                    .map(|parent| parent.to_path_buf())
            } else {
                None
            }
        })
    })
}

pub struct Pixi {}

impl Pixi {
    pub fn new() -> Pixi {
        Pixi {}
    }
}
impl Default for Pixi {
    fn default() -> Self {
        Self::new()
    }
}

impl Locator for Pixi {
    fn get_kind(&self) -> LocatorKind {
        LocatorKind::Pixi
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::Pixi]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        get_pixi_prefix(env).and_then(|prefix| {
            if !is_pixi_env(&prefix) {
                return None;
            }

            let name = prefix
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();

            let symlinks = find_executables(&prefix);

            let version = CondaPackageInfo::from(&prefix, &Package::Python)
                .map(|package_info| package_info.version);

            Some(
                PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::Pixi))
                    .executable(Some(env.executable.clone()))
                    .name(Some(name))
                    .prefix(Some(prefix))
                    .symlinks(Some(symlinks))
                    .version(version)
                    .build(),
            )
        })
    }

    fn find(&self, _reporter: &dyn Reporter) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn create_test_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("pet-pixi-{name}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn create_pixi_prefix() -> PathBuf {
        let prefix = create_test_dir("prefix").join("pixi-env");
        fs::create_dir_all(prefix.join("conda-meta")).unwrap();
        fs::write(prefix.join("conda-meta").join("pixi"), b"").unwrap();
        fs::create_dir_all(prefix.join(if cfg!(windows) { "Scripts" } else { "bin" })).unwrap();
        prefix
    }

    #[test]
    fn pixi_locator_reports_kind_and_supported_category() {
        let locator = Pixi::default();

        assert_eq!(locator.get_kind(), LocatorKind::Pixi);
        assert_eq!(
            locator.supported_categories(),
            vec![PythonEnvironmentKind::Pixi]
        );
    }

    #[test]
    fn is_pixi_env_checks_for_pixi_marker_file() {
        let prefix = create_pixi_prefix();

        assert!(is_pixi_env(&prefix));
        assert!(!is_pixi_env(&prefix.join("conda-meta")));

        fs::remove_dir_all(prefix.parent().unwrap()).unwrap();
    }

    #[test]
    fn try_from_identifies_pixi_env_from_explicit_prefix() {
        let prefix = create_pixi_prefix();
        let executable = prefix
            .join(if cfg!(windows) { "Scripts" } else { "bin" })
            .join(if cfg!(windows) {
                "python.exe"
            } else {
                "python"
            });
        fs::write(&executable, b"").unwrap();
        let locator = Pixi::new();
        let env = PythonEnv::new(
            executable.clone(),
            Some(prefix.clone()),
            Some("3.12.0".to_string()),
        );

        let pixi_env = locator.try_from(&env).unwrap();

        assert_eq!(pixi_env.kind, Some(PythonEnvironmentKind::Pixi));
        assert_eq!(pixi_env.name, Some("pixi-env".to_string()));
        assert_eq!(pixi_env.prefix, Some(prefix.clone()));
        assert_eq!(pixi_env.executable, Some(executable));

        fs::remove_dir_all(prefix.parent().unwrap()).unwrap();
    }

    #[test]
    fn try_from_derives_pixi_prefix_from_nested_python_executable() {
        let prefix = create_pixi_prefix();
        let executable = prefix
            .join(if cfg!(windows) { "Scripts" } else { "bin" })
            .join(if cfg!(windows) {
                "python.exe"
            } else {
                "python"
            });
        fs::write(&executable, b"").unwrap();
        let locator = Pixi::new();
        let env = PythonEnv::new(executable, None, None);

        let pixi_env = locator.try_from(&env).unwrap();

        assert_eq!(pixi_env.kind, Some(PythonEnvironmentKind::Pixi));
        assert_eq!(pixi_env.prefix, Some(prefix.clone()));

        fs::remove_dir_all(prefix.parent().unwrap()).unwrap();
    }

    #[test]
    fn try_from_rejects_non_pixi_environments() {
        let prefix = create_test_dir("plain-prefix");
        let executable = prefix.join("python");
        fs::write(&executable, b"").unwrap();
        let locator = Pixi::new();
        let env = PythonEnv::new(executable, Some(prefix.clone()), None);

        assert!(locator.try_from(&env).is_none());

        fs::remove_dir_all(prefix).unwrap();
    }
}
