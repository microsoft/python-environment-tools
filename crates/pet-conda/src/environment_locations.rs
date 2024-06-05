// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use crate::{
    conda_rc::Condarc,
    env_variables::EnvVariables,
    utils::{is_conda_env, is_conda_install},
};
use log::trace;
use std::{
    fs,
    path::{Path, PathBuf},
    thread,
};

pub fn get_conda_environment_paths(env_vars: &EnvVariables) -> Vec<PathBuf> {
    let mut env_paths = thread::scope(|s| {
        let mut envs = vec![];
        for thread in [
            s.spawn(|| {
                get_conda_envs_from_environment_txt(env_vars)
                    .iter()
                    .map(PathBuf::from)
                    .collect::<Vec<PathBuf>>()
            }),
            s.spawn(|| get_conda_environment_paths_from_conda_rc(env_vars)),
            s.spawn(|| get_conda_environment_paths_from_known_paths(env_vars)),
            s.spawn(|| get_known_conda_install_locations(env_vars)),
        ] {
            if let Ok(mut env_paths) = thread.join() {
                envs.append(&mut env_paths);
            }
        }
        envs
    });

    env_paths.sort();
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

    result.sort();
    result.dedup();
    result
}

/**
 * Get the list of conda environments found in other locations such as
 * <user home>/.conda/envs
 * <user home>/AppData/Local/conda/conda/envs
 */
fn get_conda_environment_paths_from_conda_rc(env_vars: &EnvVariables) -> Vec<PathBuf> {
    if let Some(conda_rc) = Condarc::from(env_vars) {
        conda_rc.env_dirs
    } else {
        vec![]
    }
}

fn get_conda_environment_paths_from_known_paths(env_vars: &EnvVariables) -> Vec<PathBuf> {
    let mut env_paths: Vec<PathBuf> = vec![];
    if let Some(ref home) = env_vars.home {
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
    env_paths.append(&mut env_vars.known_global_search_locations.clone());
    env_paths
}

pub fn get_environments(conda_dir: &Path) -> Vec<PathBuf> {
    let mut envs: Vec<PathBuf> = vec![];

    if is_conda_install(conda_dir) {
        envs.push(conda_dir.to_path_buf());

        if let Ok(entries) = fs::read_dir(conda_dir.join("envs")) {
            envs.append(
                &mut entries
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| is_conda_env(p))
                    .collect(),
            );
        }
    } else if is_conda_env(conda_dir) {
        envs.push(conda_dir.to_path_buf());
    } else if fs::metadata(conda_dir.join("envs")).is_ok() {
        // This could be a directory where conda environments are stored.
        // I.e. its not necessarily the root conda install directory.
        // E.g. C:\Users\donjayamanne\.conda
        if let Ok(entries) = fs::read_dir(conda_dir.join("envs")) {
            envs.append(
                &mut entries
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| is_conda_env(p))
                    .collect(),
            );
        }
    }

    envs.sort();
    envs.dedup();
    envs
}

pub fn get_conda_envs_from_environment_txt(env_vars: &EnvVariables) -> Vec<PathBuf> {
    let mut envs: Vec<PathBuf> = vec![];
    if let Some(ref home) = env_vars.home {
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
pub fn get_known_conda_install_locations(env_vars: &EnvVariables) -> Vec<PathBuf> {
    let user_profile = env_vars.userprofile.clone().unwrap_or_default();
    let program_data = env_vars.programdata.clone().unwrap_or_default();
    let all_user_profile = env_vars.allusersprofile.clone().unwrap_or_default();
    let mut home_drive = env_vars.homedrive.clone().unwrap_or_default();
    let mut known_paths = vec![];
    for env_variable in &[program_data, all_user_profile, user_profile] {
        if !env_variable.is_empty() {
            known_paths.push(Path::new(&env_variable).join("anaconda3"));
            known_paths.push(Path::new(&env_variable).join("miniconda3"));
            known_paths.push(Path::new(&env_variable).join("miniforge3"));
        }
    }
    if !home_drive.is_empty() {
        if home_drive.ends_with(':') {
            home_drive = format!("{}\\", home_drive);
        }
        known_paths.push(Path::new(&home_drive).join("anaconda3"));
        known_paths.push(Path::new(&home_drive).join("miniconda"));
        known_paths.push(Path::new(&home_drive).join("miniforge3"));
    }
    if let Some(ref conda_root) = env_vars.conda_root {
        known_paths.push(PathBuf::from(conda_root.clone()));
    }
    if let Some(ref conda_prefix) = env_vars.conda_prefix {
        known_paths.push(PathBuf::from(conda_prefix.clone()));
    }
    if let Some(ref conda) = env_vars.conda {
        let conda = PathBuf::from(conda);
        if let Some(parent) = conda.parent() {
            known_paths.push(parent.to_path_buf());
        }
    }
    if let Some(home) = env_vars.clone().home {
        known_paths.push(home.clone().join("anaconda3"));
        known_paths.push(home.clone().join("miniconda3"));
        known_paths.push(home.clone().join("miniforge3"));
        // E.g. C:\Users\user name\.conda where we have `envs`` under this directory.
        known_paths.push(home.join(".conda"));
        // E.g. C:\Users\user name\AppData\Local\conda\conda\envs
        known_paths.push(
            home.join("AppData")
                .join("Local")
                .join("conda")
                .join("conda"),
        );
    }
    known_paths.sort();
    known_paths.dedup();
    known_paths
}

#[cfg(unix)]
pub fn get_known_conda_install_locations(env_vars: &EnvVariables) -> Vec<PathBuf> {
    let mut known_paths = vec![];
    let directories_to_look_in = [
        "/opt",
        "/opt",
        "/usr/share",
        "/usr/local",
        "/usr",
        "/home",
        "", // We need to look in `/anaconda3` and `/miniconda3` as well.
    ];
    for directory in directories_to_look_in.iter() {
        known_paths.push(PathBuf::from(format!("{}/anaconda", directory)));
        known_paths.push(PathBuf::from(format!("{}/anaconda3", directory)));
        known_paths.push(PathBuf::from(format!("{}/miniconda", directory)));
        known_paths.push(PathBuf::from(format!("{}/miniconda3", directory)));
        known_paths.push(PathBuf::from(format!("{}/miniforge", directory)));
        known_paths.push(PathBuf::from(format!("{}/miniforge3", directory)));
    }
    if let Some(ref conda_root) = env_vars.conda_root {
        known_paths.push(PathBuf::from(conda_root.clone()));
    }
    if let Some(ref conda_prefix) = env_vars.conda_prefix {
        known_paths.push(PathBuf::from(conda_prefix.clone()));
    }
    if let Some(ref conda) = env_vars.conda {
        let conda = PathBuf::from(conda);
        if let Some(parent) = conda.parent() {
            known_paths.push(parent.to_path_buf());
        }
    }
    if let Some(ref home) = env_vars.home {
        known_paths.push(home.clone().join("anaconda"));
        known_paths.push(home.clone().join("anaconda3"));
        known_paths.push(home.clone().join("miniconda"));
        known_paths.push(home.clone().join("miniconda3"));
        known_paths.push(home.clone().join("miniforge3"));
        known_paths.push(home.join(".conda"));
    }
    known_paths.append(get_known_conda_locations(env_vars).as_mut());
    known_paths.sort();
    known_paths.dedup();
    known_paths
}

#[cfg(windows)]
pub fn get_known_conda_locations(env_vars: &EnvVariables) -> Vec<PathBuf> {
    let user_profile = env_vars.userprofile.clone().unwrap_or_default();
    let program_data = env_vars.programdata.clone().unwrap_or_default();
    let all_user_profile = env_vars.allusersprofile.clone().unwrap_or_default();
    let home_drive = env_vars.homedrive.clone().unwrap_or_default();
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
    known_paths.append(&mut env_vars.known_global_search_locations.clone());
    known_paths
}

#[cfg(unix)]
pub fn get_known_conda_locations(env_vars: &EnvVariables) -> Vec<PathBuf> {
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
    if let Some(ref home) = env_vars.home {
        known_paths.push(home.clone().join("anaconda3/bin"));
        known_paths.push(home.join("miniconda3/bin"));
    }
    known_paths.append(&mut env_vars.known_global_search_locations.clone());
    known_paths
}
