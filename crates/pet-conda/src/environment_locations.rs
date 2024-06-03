// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use crate::{
    conda_rc::get_conda_conda_rc,
    utils::{is_conda_env, is_conda_install},
};
use log::trace;
use pet_core::os_environment::Environment;
use std::{
    fs,
    path::{Path, PathBuf},
};

// use super::conda_rc::get_conda_conda_rc;
// use crate::{
//     conda::{is_conda_env_location, is_conda_install_location},
//     known::Environment,
// };
// use log::trace;
// use std::{
//     collections::HashSet,
//     path::{Path, PathBuf},
// };

pub fn get_conda_environment_paths(environment: &dyn Environment) -> Vec<PathBuf> {
    let mut env_paths = get_conda_envs_from_environment_txt(environment)
        .iter()
        .map(|e| PathBuf::from(e))
        .collect::<Vec<PathBuf>>();

    let mut env_paths_from_conda_rc = get_conda_environment_paths_from_conda_rc(environment);
    env_paths.append(&mut env_paths_from_conda_rc);

    let mut envs_from_known_paths = get_conda_environment_paths_from_known_paths(environment);
    env_paths.append(&mut envs_from_known_paths);

    let mut envs_from_known_paths = get_known_conda_install_locations(environment);
    env_paths.append(&mut envs_from_known_paths);

    // For each env, check if we have a conda install directory in them and
    // & then iterate through the list of envs in the envs directory.
    for env_path in env_paths.clone().iter().filter(|e| is_conda_env(e)) {
        let envs = get_environments_in_conda_dir(&env_path);
        env_paths.extend(envs);
    }

    // Remove duplicates.
    env_paths.dedup();
    env_paths
}

/**
 * Get the list of conda environments found in other locations such as
 * <user home>/.conda/envs
 * <user home>/AppData/Local/conda/conda/envs
 */
pub fn get_conda_environment_paths_from_conda_rc(environment: &dyn Environment) -> Vec<PathBuf> {
    if let Some(paths) = get_conda_conda_rc(environment) {
        paths.env_dirs
    } else {
        vec![]
    }
}

pub fn get_conda_environment_paths_from_known_paths(environment: &dyn Environment) -> Vec<PathBuf> {
    let mut env_paths: Vec<PathBuf> = vec![];
    if let Some(home) = environment.get_user_home() {
        let known_conda_paths = [
            PathBuf::from(".conda").join("envs"),
            PathBuf::from("AppData")
                .join("Local")
                .join("conda")
                .join("conda")
                .join("envs"),
        ];
        for path in known_conda_paths {
            // We prefix with home only for testing purposes.
            let full_path = home.join(path);
            if let Ok(entries) = fs::read_dir(full_path) {
                for entry in entries.filter_map(Result::ok) {
                    let path = entry.path();
                    if let Some(meta) = fs::metadata(&path).ok() {
                        if meta.is_dir() {
                            env_paths.push(path);
                        }
                    }
                }
            }
        }
    }
    return env_paths;
}

pub fn get_environments_in_conda_dir(conda_dir: &Path) -> Vec<PathBuf> {
    let mut envs: Vec<PathBuf> = vec![];

    if is_conda_install(conda_dir) {
        envs.push(conda_dir.to_path_buf());

        if let Ok(entries) = fs::read_dir(conda_dir.join("envs")) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if is_conda_env(&path) {
                    envs.push(path);
                }
            }
        }
    } else if is_conda_env(conda_dir) {
        envs.push(conda_dir.to_path_buf());
    }

    envs.dedup();
    envs
}

pub fn get_conda_envs_from_environment_txt(environment: &dyn Environment) -> Vec<PathBuf> {
    let mut envs: Vec<PathBuf> = vec![];
    if let Some(home) = environment.get_user_home() {
        let home = Path::new(&home);
        let environment_txt = home.join(".conda").join("environments.txt");
        if let Ok(reader) = fs::read_to_string(environment_txt.clone()) {
            trace!("Found environments.txt file {:?}", environment_txt);
            for line in reader.lines() {
                envs.push(PathBuf::from(line.to_string()));
            }
        }
    }

    envs
}

#[cfg(windows)]
pub fn get_known_conda_install_locations(environment: &dyn Environment) -> Vec<PathBuf> {
    let user_profile = environment.get_env_var("USERPROFILE".to_string()).unwrap();
    let program_data = environment.get_env_var("PROGRAMDATA".to_string()).unwrap();
    let all_user_profile = environment
        .get_env_var("ALLUSERSPROFILE".to_string())
        .unwrap();
    let home_drive = environment.get_env_var("HOMEDRIVE".to_string()).unwrap();
    let mut known_paths = vec![
        Path::new(&user_profile).join("Anaconda3"),
        Path::new(&program_data).join("Anaconda3"),
        Path::new(&all_user_profile).join("Anaconda3"),
        Path::new(&home_drive).join("Anaconda3"),
        Path::new(&user_profile).join("Miniconda3"),
        Path::new(&program_data).join("Miniconda3"),
        Path::new(&all_user_profile).join("Miniconda3"),
        Path::new(&home_drive).join("Miniconda3"),
        Path::new(&all_user_profile).join("miniforge3"),
        Path::new(&home_drive).join("miniforge3"),
    ];
    if let Some(home) = environment.get_user_home() {
        known_paths.push(PathBuf::from(home.clone()).join("anaconda3"));
        known_paths.push(PathBuf::from(home.clone()).join("miniconda3"));
        known_paths.push(PathBuf::from(home.clone()).join("miniforge3"));
        known_paths.push(PathBuf::from(home).join(".conda"));
    }
    known_paths
}

#[cfg(unix)]
pub fn get_known_conda_install_locations(environment: &dyn Environment) -> Vec<PathBuf> {
    let mut known_paths = vec![
        PathBuf::from("/opt/anaconda3"),
        PathBuf::from("/opt/miniconda3"),
        PathBuf::from("/usr/local/anaconda3"),
        PathBuf::from("/usr/local/miniconda3"),
        PathBuf::from("/usr/anaconda3"),
        PathBuf::from("/usr/miniconda3"),
        PathBuf::from("/home/anaconda3"),
        PathBuf::from("/home/miniconda3"),
        PathBuf::from("/anaconda3"),
        PathBuf::from("/miniconda3"),
        PathBuf::from("/miniforge3"),
        PathBuf::from("/miniforge3"),
    ];
    if let Some(home) = environment.get_user_home() {
        known_paths.push(PathBuf::from(home.clone()).join("anaconda3"));
        known_paths.push(PathBuf::from(home.clone()).join("miniconda3"));
        known_paths.push(PathBuf::from(home.clone()).join("miniforge3"));
        known_paths.push(PathBuf::from(home).join(".conda"));
    }
    known_paths
}

#[cfg(windows)]
pub fn get_known_conda_locations(environment: &dyn Environment) -> Vec<PathBuf> {
    let user_profile = environment.get_env_var("USERPROFILE".to_string()).unwrap();
    let program_data = environment.get_env_var("PROGRAMDATA".to_string()).unwrap();
    let all_user_profile = environment
        .get_env_var("ALLUSERSPROFILE".to_string())
        .unwrap();
    let home_drive = environment.get_env_var("HOMEDRIVE".to_string()).unwrap();
    let mut known_paths = vec![
        Path::new(&user_profile).join("Anaconda3\\Scripts"),
        Path::new(&program_data).join("Anaconda3\\Scripts"),
        Path::new(&all_user_profile).join("Anaconda3\\Scripts"),
        Path::new(&home_drive).join("Anaconda3\\Scripts"),
        Path::new(&user_profile).join("Miniconda3\\Scripts"),
        Path::new(&program_data).join("Miniconda3\\Scripts"),
        Path::new(&all_user_profile).join("Miniconda3\\Scripts"),
        Path::new(&home_drive).join("Miniconda3\\Scripts"),
    ];
    known_paths.append(&mut environment.get_know_global_search_locations());
    known_paths
}

#[cfg(unix)]
pub fn get_known_conda_locations(environment: &dyn Environment) -> Vec<PathBuf> {
    let mut known_paths = vec![
        PathBuf::from("/opt/anaconda3/bin"),
        PathBuf::from("/opt/miniconda3/bin"),
        PathBuf::from("/usr/local/anaconda3/bin"),
        PathBuf::from("/usr/local/miniconda3/bin"),
        PathBuf::from("/usr/anaconda3/bin"),
        PathBuf::from("/usr/miniconda3/bin"),
        PathBuf::from("/home/anaconda3/bin"),
        PathBuf::from("/home/miniconda3/bin"),
        PathBuf::from("/anaconda3/bin"),
        PathBuf::from("/miniconda3/bin"),
    ];
    if let Some(home) = environment.get_user_home() {
        known_paths.push(PathBuf::from(home.clone()).join("anaconda3/bin"));
        known_paths.push(PathBuf::from(home).join("miniconda3/bin"));
    }
    known_paths.append(&mut environment.get_know_global_search_locations());
    known_paths
}
