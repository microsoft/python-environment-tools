// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use crate::{
    environment_locations::get_known_conda_locations, package::CondaPackageInfo,
    utils::CondaEnvironmentVariables,
};
use log::warn;
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
        if exe.metadata().is_ok() {
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
pub fn find_conda_binary_on_path(environment: &CondaEnvironmentVariables) -> Option<PathBuf> {
    let paths = environment.path.clone()?;
    for path in env::split_paths(&paths) {
        for bin in get_conda_bin_names() {
            let conda_path = path.join(bin);
            if let Ok(metadata) = std::fs::metadata(&conda_path) {
                if metadata.is_file() || metadata.is_symlink() {
                    return Some(conda_path);
                }
            }
        }
    }
    None
}

/// Find conda binary in known locations
fn find_conda_binary_in_known_locations(
    environment: &CondaEnvironmentVariables,
) -> Option<PathBuf> {
    let conda_bin_names = get_conda_bin_names();
    let known_locations = get_known_conda_locations(environment);
    for location in known_locations {
        for bin in &conda_bin_names {
            let conda_path = location.join(bin);
            if let Ok(metadata) = std::fs::metadata(&conda_path) {
                if metadata.is_file() || metadata.is_symlink() {
                    return Some(conda_path);
                }
            }
        }
    }
    None
}

/// Find the conda binary on the system
pub fn find_conda_binary(environment: &CondaEnvironmentVariables) -> Option<PathBuf> {
    let conda_binary_on_path = find_conda_binary_on_path(environment);
    match conda_binary_on_path {
        Some(conda_binary_on_path) => Some(conda_binary_on_path),
        None => find_conda_binary_in_known_locations(environment),
    }
}

#[derive(Debug, Clone)]
pub struct CondaManager {
    pub executable: PathBuf,
    pub version: Option<String>,
    pub company: Option<String>,
    pub company_display_name: Option<String>,
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
}

pub fn get_conda_manager(path: &Path) -> Option<CondaManager> {
    let conda_exe = get_conda_executable(path)?;
    if let Some(conda_pkg) = CondaPackageInfo::from(path, &crate::package::Package::Conda) {
        Some(CondaManager {
            executable: conda_exe,
            version: Some(conda_pkg.version),
            company: None,
            company_display_name: None,
            conda_dir: path.to_path_buf(),
        })
    } else {
        warn!("Could not get conda package info from {:?}", path);
        None
    }
}

// pub fn get_conda_version(conda_binary: &PathBuf) -> Option<String> {
//     let mut parent = conda_binary.parent()?;
//     if parent.ends_with("bin") {
//         parent = parent.parent()?;
//     }
//     if parent.ends_with("Library") {
//         parent = parent.parent()?;
//     }
//     match get_conda_package_info(&parent, "conda") {
//         Some(result) => Some(result.version),
//         None => match get_conda_package_info(&parent.parent()?, "conda") {
//             Some(result) => Some(result.version),
//             None => None,
//         },
//     }
// }
