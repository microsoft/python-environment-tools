// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use crate::{
    conda_rc::Condarc,
    utils::{is_conda_env, is_conda_install, CondaEnvironmentVariables},
};
use log::trace;
use std::{
    fs,
    path::{Path, PathBuf},
    thread,
};

pub fn get_conda_environment_paths(environment: &CondaEnvironmentVariables) -> Vec<PathBuf> {
    let mut env_paths = get_conda_envs_from_environment_txt(environment)
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<PathBuf>>();

    let mut env_paths_from_conda_rc = get_conda_environment_paths_from_conda_rc(environment);
    env_paths.append(&mut env_paths_from_conda_rc);

    let mut envs_from_known_paths = get_conda_environment_paths_from_known_paths(environment);
    env_paths.append(&mut envs_from_known_paths);

    let mut envs_from_known_paths = get_known_conda_install_locations(environment);
    env_paths.append(&mut envs_from_known_paths);

    env_paths.dedup();

    // For each env, check if we have a conda install directory in them and
    // & then iterate through the list of envs in the envs directory.
    // let env_paths = vec![];
    let mut threads = vec![];
    for path in env_paths {
        let path = path.clone();
        threads.push(thread::spawn(move || get_environments(&path)));
    }

    let mut result = vec![];
    for thread in threads {
        if let Ok(envs) = thread.join() {
            result.extend(envs);
        }
    }

    result.dedup();
    result
}

/**
 * Get the list of conda environments found in other locations such as
 * <user home>/.conda/envs
 * <user home>/AppData/Local/conda/conda/envs
 */
pub fn get_conda_environment_paths_from_conda_rc(
    environment: &CondaEnvironmentVariables,
) -> Vec<PathBuf> {
    if let Some(conda_rc) = Condarc::from(environment) {
        conda_rc.env_dirs
    } else {
        vec![]
    }
}

pub fn get_conda_environment_paths_from_known_paths(
    environment: &CondaEnvironmentVariables,
) -> Vec<PathBuf> {
    let mut env_paths: Vec<PathBuf> = vec![];
    if let Some(ref home) = environment.home {
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
                    if let Ok(meta) = fs::metadata(&path) {
                        if meta.is_dir() {
                            env_paths.push(path);
                        }
                    }
                }
            }
        }
    }
    env_paths
}

pub fn get_environments(conda_dir: &Path) -> Vec<PathBuf> {
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

pub fn get_conda_envs_from_environment_txt(
    environment: &CondaEnvironmentVariables,
) -> Vec<PathBuf> {
    let mut envs: Vec<PathBuf> = vec![];
    if let Some(ref home) = environment.home {
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
pub fn get_known_conda_install_locations(environment: &CondaEnvironmentVariables) -> Vec<PathBuf> {
    let user_profile = environment.userprofile.clone().unwrap_or_default();
    let program_data = environment.programdata.clone().unwrap_or_default();
    let all_user_profile = environment.allusersprofile.clone().unwrap_or_default();
    let home_drive = environment.homedrive.clone().unwrap_or_default();
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
    if let Some(home) = environment.clone().home {
        known_paths.push(PathBuf::from(home.clone()).join("anaconda3"));
        known_paths.push(PathBuf::from(home.clone()).join("miniconda3"));
        known_paths.push(PathBuf::from(home.clone()).join("miniforge3"));
        known_paths.push(PathBuf::from(home).join(".conda"));
    }
    known_paths
}

#[cfg(unix)]
pub fn get_known_conda_install_locations(environment: &CondaEnvironmentVariables) -> Vec<PathBuf> {
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
    if let Some(ref home) = environment.home {
        known_paths.push(home.clone().join("anaconda3"));
        known_paths.push(home.clone().join("miniconda3"));
        known_paths.push(home.clone().join("miniforge3"));
        known_paths.push(home.join(".conda"));
    }
    known_paths.append(get_known_conda_locations(environment).as_mut());
    known_paths.dedup();
    known_paths
}

#[cfg(windows)]
pub fn get_known_conda_locations(environment: &CondaEnvironmentVariables) -> Vec<PathBuf> {
    let user_profile = environment.userprofile.clone().unwrap_or_default();
    let program_data = environment.programdata.clone().unwrap_or_default();
    let all_user_profile = environment.allusersprofile.clone().unwrap_or_default();
    let home_drive = environment.homedrive.clone().unwrap_or_default();
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
    known_paths.append(&mut environment.known_global_search_locations.clone());
    known_paths
}

#[cfg(unix)]
pub fn get_known_conda_locations(environment: &CondaEnvironmentVariables) -> Vec<PathBuf> {
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
    if let Some(ref home) = environment.home {
        known_paths.push(home.clone().join("anaconda3/bin"));
        known_paths.push(home.join("miniconda3/bin"));
    }
    known_paths.append(&mut environment.known_global_search_locations.clone());
    known_paths
}
