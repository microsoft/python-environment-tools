// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use crate::{
    conda_info::CondaInfo, env_variables::EnvVariables,
    environments::get_conda_installation_used_to_create_conda_env, package::CondaPackageInfo,
    utils::is_conda_env,
};
use pet_core::{manager::EnvManager, manager::EnvManagerType};
use std::{
    env,
    path::{Path, PathBuf},
};

fn get_conda_executable(path: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    let relative_path_to_conda_exe = vec![
        PathBuf::from("Scripts").join("conda.exe"),
        PathBuf::from("Scripts").join("conda.bat"),
    ];
    #[cfg(unix)]
    let relative_path_to_conda_exe = vec![PathBuf::from("bin").join("conda")];

    for relative_path in relative_path_to_conda_exe {
        let exe = path.join(&relative_path);
        if exe.exists() {
            return Some(exe);
        }
    }

    None
}

/// Specifically returns the file names that are valid for 'conda' on windows
#[cfg(windows)]
fn get_conda_bin_names() -> Vec<&'static str> {
    vec!["conda.exe", "conda.bat"]
}

/// Specifically returns the file names that are valid for 'conda' on linux/Mac
#[cfg(unix)]
fn get_conda_bin_names() -> Vec<&'static str> {
    vec!["conda"]
}

/// Find the conda binary on the PATH environment variable
pub fn find_conda_binary(env_vars: &EnvVariables) -> Option<PathBuf> {
    let paths = env_vars.path.clone()?;
    for path in env::split_paths(&paths) {
        for bin in get_conda_bin_names() {
            let conda_path = path.join(bin);
            if conda_path.is_file() || conda_path.is_symlink() {
                return Some(conda_path);
            }
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct CondaManager {
    pub executable: PathBuf,
    pub version: Option<String>,
    pub conda_dir: PathBuf,
}

impl CondaManager {
    pub fn to_manager(&self) -> EnvManager {
        EnvManager {
            tool: EnvManagerType::Conda,
            executable: self.executable.clone(),
            version: self.version.clone(),
        }
    }
    pub fn from(path: &Path) -> Option<CondaManager> {
        if !is_conda_env(path) {
            return None;
        }
        if let Some(manager) = get_conda_manager(path) {
            Some(manager)
        } else {
            // Possible this is a conda environment in the `envs` folder
            let path = path.parent()?.parent()?;
            if let Some(manager) = get_conda_manager(path) {
                Some(manager)
            } else {
                // Possible this is a conda environment in some other location
                // Such as global env folders location configured via condarc file
                // Or a conda env created using `-p` flag.
                // Get the conda install folder from the history file.
                let conda_install_folder = get_conda_installation_used_to_create_conda_env(path)?;
                get_conda_manager(&conda_install_folder)
            }
        }
    }
    pub fn from_info(executable: &Path, info: &CondaInfo) -> Option<CondaManager> {
        Some(CondaManager {
            executable: executable.to_path_buf(),
            version: Some(info.conda_version.clone()),
            conda_dir: info.conda_prefix.clone(),
        })
    }
}

fn get_conda_manager(path: &Path) -> Option<CondaManager> {
    let conda_exe = get_conda_executable(path)?;
    if let Some(conda_pkg) = CondaPackageInfo::from(path, &crate::package::Package::Conda) {
        Some(CondaManager {
            executable: conda_exe,
            version: Some(conda_pkg.version),
            conda_dir: path.to_path_buf(),
        })
    } else {
        None
    }
}
