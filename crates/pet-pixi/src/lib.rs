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
