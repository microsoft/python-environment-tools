// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use log::trace;
use pet_core::os_environment::Environment;
use std::{fs, path::PathBuf};

#[derive(Debug)]
pub struct Condarc {
    pub env_dirs: Vec<PathBuf>,
}

#[cfg(windows)]
fn get_conda_rc_search_paths(environment: &dyn Environment) -> Vec<PathBuf> {
    let mut search_paths: Vec<PathBuf> = vec![
        "C:\\ProgramData\\conda\\.condarc",
        "C:\\ProgramData\\conda\\condarc",
        "C:\\ProgramData\\conda\\condarc.d",
    ]
    .iter()
    .map(|p| PathBuf::from(p))
    .collect();

    if let Some(conda_root) = environment.get_env_var("CONDA_ROOT".to_string()) {
        search_paths.append(&mut vec![
            PathBuf::from(conda_root.clone()).join(".condarc"),
            PathBuf::from(conda_root.clone()).join("condarc"),
            PathBuf::from(conda_root.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(home) = environment.get_user_home() {
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
    if let Some(conda_prefix) = environment.get_env_var("CONDA_PREFIX".to_string()) {
        search_paths.append(&mut vec![
            PathBuf::from(conda_prefix.clone()).join(".condarc"),
            PathBuf::from(conda_prefix.clone()).join("condarc"),
            PathBuf::from(conda_prefix.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(condarc) = environment.get_env_var("CONDARC".to_string()) {
        search_paths.append(&mut vec![PathBuf::from(condarc)]);
    }

    search_paths
}
#[cfg(unix)]
fn get_conda_rc_search_paths(environment: &dyn Environment) -> Vec<PathBuf> {
    let mut search_paths: Vec<PathBuf> = vec![
        "/etc/conda/.condarc",
        "/etc/conda/condarc",
        "/etc/conda/condarc.d/",
        "/var/lib/conda/.condarc",
        "/var/lib/conda/condarc",
        "/var/lib/conda/condarc.d/",
    ]
    .iter()
    .map(|p| PathBuf::from(p))
    .map(|p| {
        // This only applies in tests.
        // We need this, as the root folder cannot be mocked.
        if let Some(root) = environment.get_root() {
            root.join(p.to_string_lossy()[1..].to_string())
        } else {
            p
        }
    })
    .collect();

    if let Some(conda_root) = environment.get_env_var("CONDA_ROOT".to_string()) {
        search_paths.append(&mut vec![
            PathBuf::from(conda_root.clone()).join(".condarc"),
            PathBuf::from(conda_root.clone()).join("condarc"),
            PathBuf::from(conda_root.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(xdg_config_home) = environment.get_env_var("XDG_CONFIG_HOME".to_string()) {
        search_paths.append(&mut vec![
            PathBuf::from(xdg_config_home.clone()).join(".condarc"),
            PathBuf::from(xdg_config_home.clone()).join("condarc"),
            PathBuf::from(xdg_config_home.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(home) = environment.get_user_home() {
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
    if let Some(conda_prefix) = environment.get_env_var("CONDA_PREFIX".to_string()) {
        search_paths.append(&mut vec![
            PathBuf::from(conda_prefix.clone()).join(".condarc"),
            PathBuf::from(conda_prefix.clone()).join("condarc"),
            PathBuf::from(conda_prefix.clone()).join(".condarc.d"),
        ]);
    }
    if let Some(condarc) = environment.get_env_var("CONDARC".to_string()) {
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
pub fn get_conda_conda_rc(environment: &dyn Environment) -> Option<Condarc> {
    let conda_rc = get_conda_rc_search_paths(environment)
        .into_iter()
        .find(|p| p.exists())?;
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
                    let env_dir = PathBuf::from(env_dir.trim()).join("envs");
                    if fs::metadata(&env_dir).is_ok() {
                        env_dirs.push(env_dir);
                    }
                }
                continue;
            } else {
                break;
            }
        }
    }
    return Some(Condarc { env_dirs });
}
