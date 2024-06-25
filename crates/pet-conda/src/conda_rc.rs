// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use crate::env_variables::EnvVariables;
use log::trace;
use pet_fs::path::norm_case;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct Condarc {
    pub env_dirs: Vec<PathBuf>,
}

impl Condarc {
    pub fn from(env_vars: &EnvVariables) -> Option<Condarc> {
        get_conda_conda_rc(env_vars)
    }
    pub fn from_path(path: &Path) -> Option<Condarc> {
        parse_conda_rc(&path.join(".condarc"))
    }
}

#[cfg(windows)]
// Search paths documented here
// https://conda.io/projects/conda/en/latest/user-guide/configuration/use-condarc.html#searching-for-condarc
fn get_conda_rc_search_paths(env_vars: &EnvVariables) -> Vec<PathBuf> {
    let mut search_paths: Vec<PathBuf> = [
        "C:\\ProgramData\\conda\\.condarc",
        "C:\\ProgramData\\conda\\condarc",
        "C:\\ProgramData\\conda\\condarc.d",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();

    if let Some(ref conda_root) = env_vars.conda_root {
        search_paths.append(&mut vec![
            PathBuf::from(conda_root.clone()).join(".condarc"),
            PathBuf::from(conda_root.clone()).join("condarc"),
            PathBuf::from(conda_root.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(ref home) = env_vars.home {
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
    if let Some(ref conda_prefix) = env_vars.conda_prefix {
        search_paths.append(&mut vec![
            PathBuf::from(conda_prefix.clone()).join(".condarc"),
            PathBuf::from(conda_prefix.clone()).join("condarc"),
            PathBuf::from(conda_prefix.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(ref condarc) = env_vars.condarc {
        search_paths.append(&mut vec![PathBuf::from(condarc)]);
    }

    // let search_paths = search_paths
    //     .into_iter()
    //     .filter(|p| p.exists())
    //     .collect::<Vec<PathBuf>>();
    search_paths
}

#[cfg(unix)]
// Search paths documented here
// https://conda.io/projects/conda/en/latest/user-guide/configuration/use-condarc.html#searching-for-condarc
fn get_conda_rc_search_paths(env_vars: &EnvVariables) -> Vec<PathBuf> {
    use crate::utils::change_root_of_path;

    let mut search_paths: Vec<PathBuf> = [
        "/etc/conda/.condarc",
        "/etc/conda/condarc",
        "/etc/conda/condarc.d",
        "/var/lib/conda/.condarc",
        "/var/lib/conda/condarc",
        "/var/lib/conda/condarc.d",
    ]
    .iter()
    .map(PathBuf::from)
    .map(|p| change_root_of_path(&p, &env_vars.root))
    .collect();

    if let Some(ref conda_root) = env_vars.conda_root {
        search_paths.append(&mut vec![
            PathBuf::from(conda_root.clone()).join(".condarc"),
            PathBuf::from(conda_root.clone()).join("condarc"),
            PathBuf::from(conda_root.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(ref xdg_config_home) = env_vars.xdg_config_home {
        search_paths.append(&mut vec![
            PathBuf::from(xdg_config_home.clone()).join(".condarc"),
            PathBuf::from(xdg_config_home.clone()).join("condarc"),
            PathBuf::from(xdg_config_home.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(ref home) = env_vars.home {
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
    if let Some(ref conda_prefix) = env_vars.conda_prefix {
        search_paths.append(&mut vec![
            PathBuf::from(conda_prefix.clone()).join(".condarc"),
            PathBuf::from(conda_prefix.clone()).join("condarc"),
            PathBuf::from(conda_prefix.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(ref condarc) = env_vars.condarc {
        search_paths.append(&mut vec![PathBuf::from(condarc)]);
    }

    // let search_paths = search_paths
    //     .into_iter()
    //     .filter(|p| p.exists())
    //     .collect::<Vec<PathBuf>>();
    search_paths
}

/**
 * The .condarc file contains a list of directories where conda environments are created.
 * https://conda.io/projects/conda/en/latest/configuration.html#envs-dirs
 *
 * TODO: Search for the .condarc file in the following locations:
 * https://conda.io/projects/conda/en/latest/user-guide/configuration/use-condarc.html#searching-for-condarc
 */
fn get_conda_conda_rc(env_vars: &EnvVariables) -> Option<Condarc> {
    let conda_rc = get_conda_rc_search_paths(env_vars)
        .into_iter()
        .find(|p: &PathBuf| p.exists())?;
    parse_conda_rc(&conda_rc)
}

fn parse_conda_rc(conda_rc: &Path) -> Option<Condarc> {
    let mut start_consuming_values = false;
    let reader = fs::read_to_string(conda_rc).ok()?;
    trace!("conda_rc: {:?}", conda_rc);
    let mut env_dirs = vec![];
    for line in reader.lines() {
        if line.starts_with("envs_dirs:") && !start_consuming_values {
            start_consuming_values = true;
            continue;
        }
        if start_consuming_values {
            if line.trim().starts_with('-') {
                if let Some(env_dir) = line.split_once('-').map(|x| x.1) {
                    // Directories in conda-rc are where `envs` are stored.
                    let path = PathBuf::from(env_dir.trim()).join("envs");
                    // if path.exists() {
                    env_dirs.push(norm_case(&path));
                    // }
                }
                continue;
            } else {
                break;
            }
        }
    }
    Some(Condarc { env_dirs })
}
