// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use crate::utils::CondaEnvironmentVariables;
use log::trace;
use std::{fs, path::PathBuf};

#[derive(Debug)]
pub struct Condarc {
    pub env_dirs: Vec<PathBuf>,
}

impl Condarc {
    pub fn from(environment: &CondaEnvironmentVariables) -> Option<Condarc> {
        get_conda_conda_rc(environment)
    }
}

#[cfg(windows)]
fn get_conda_rc_search_paths(environment: &CondaEnvironmentVariables) -> Vec<PathBuf> {
    let mut search_paths: Vec<PathBuf> = vec![
        "C:\\ProgramData\\conda\\.condarc",
        "C:\\ProgramData\\conda\\condarc",
        "C:\\ProgramData\\conda\\condarc.d",
    ]
    .iter()
    .map(|p| PathBuf::from(p))
    .collect();

    if let Some(ref conda_root) = environment.conda_root {
        search_paths.append(&mut vec![
            PathBuf::from(conda_root.clone()).join(".condarc"),
            PathBuf::from(conda_root.clone()).join("condarc"),
            PathBuf::from(conda_root.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(ref home) = environment.home {
        search_paths.append(&mut vec![
            home.join(".config").join("conda").join(".condarc"),
            home.join(".config").join("conda").join("condarc"),
            home.join(".config").join("conda").join("condarc.d"),
            home.join(".conda").join(".condarc"),
            home.join(".conda").join("condarc"),
            home.join(".conda").join("condarc.d"),
            home.join(".condarc"),
        ]);
    }
    if let Some(ref conda_prefix) = environment.conda_prefix {
        search_paths.append(&mut vec![
            PathBuf::from(conda_prefix.clone()).join(".condarc"),
            PathBuf::from(conda_prefix.clone()).join("condarc"),
            PathBuf::from(conda_prefix.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(ref condarc) = environment.condarc {
        search_paths.append(&mut vec![PathBuf::from(condarc)]);
    }

    search_paths
}

#[cfg(unix)]
fn get_conda_rc_search_paths(environment: &CondaEnvironmentVariables) -> Vec<PathBuf> {
    let mut search_paths: Vec<PathBuf> = vec![
        "/etc/conda/.condarc",
        "/etc/conda/condarc",
        "/etc/conda/condarc.d",
        "/var/lib/conda/.condarc",
        "/var/lib/conda/condarc",
        "/var/lib/conda/condarc.d",
    ]
    .iter()
    .map(|p| PathBuf::from(p))
    .map(|p| {
        // This only applies in tests.
        // We need this, as the root folder cannot be mocked.
        if let Some(ref root) = environment.root {
            // Strip the first `/` (this path is only for testing purposes)
            root.join(p.to_string_lossy()[1..].to_string())
        } else {
            p
        }
    })
    .collect();

    if let Some(ref conda_root) = environment.conda_root {
        search_paths.append(&mut vec![
            PathBuf::from(conda_root.clone()).join(".condarc"),
            PathBuf::from(conda_root.clone()).join("condarc"),
            PathBuf::from(conda_root.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(ref xdg_config_home) = environment.xdg_config_home {
        search_paths.append(&mut vec![
            PathBuf::from(xdg_config_home.clone()).join(".condarc"),
            PathBuf::from(xdg_config_home.clone()).join("condarc"),
            PathBuf::from(xdg_config_home.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(ref home) = environment.home {
        search_paths.append(&mut vec![
            home.join(".config").join("conda").join(".condarc"),
            home.join(".config").join("conda").join("condarc"),
            home.join(".config").join("conda").join("condarc.d"),
            home.join(".conda").join(".condarc"),
            home.join(".conda").join("condarc"),
            home.join(".conda").join("condarc.d"),
            home.join(".condarc"),
        ]);
    }
    if let Some(ref conda_prefix) = environment.conda_prefix {
        search_paths.append(&mut vec![
            PathBuf::from(conda_prefix.clone()).join(".condarc"),
            PathBuf::from(conda_prefix.clone()).join("condarc"),
            PathBuf::from(conda_prefix.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(ref condarc) = environment.condarc {
        search_paths.append(&mut vec![PathBuf::from(condarc)]);
    }

    search_paths
}

/**
 * The .condarc file contains a list of directories where conda environments are created.
 * https://conda.io/projects/conda/en/latest/configuration.html#envs-dirs
 *
 * TODO: Search for the .condarc file in the following locations:
 * https://conda.io/projects/conda/en/latest/user-guide/configuration/use-condarc.html#searching-for-condarc
 */
fn get_conda_conda_rc(environment: &CondaEnvironmentVariables) -> Option<Condarc> {
    let conda_rc = get_conda_rc_search_paths(environment)
        .into_iter()
        .find(|p: &PathBuf| p.exists())?;
    parse_conda_rc(&conda_rc)
}

fn parse_conda_rc(conda_rc: &PathBuf) -> Option<Condarc> {
    let mut start_consuming_values = false;
    trace!("conda_rc: {:?}", conda_rc);
    let reader = fs::read_to_string(conda_rc).ok()?;
    let mut env_dirs = vec![];
    for line in reader.lines() {
        if line.starts_with("envs_dirs:") && !start_consuming_values {
            start_consuming_values = true;
            continue;
        }
        if start_consuming_values {
            if line.trim().starts_with("-") {
                if let Some(env_dir) = line.splitn(2, '-').nth(1) {
                    // Directories in conda-rc are where `envs` are stored.
                    env_dirs.push(PathBuf::from(env_dir.trim()).join("envs"));
                }
                continue;
            } else {
                break;
            }
        }
    }
    return Some(Condarc { env_dirs });
}
