// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use crate::{
    conda_rc::Condarc,
    env_variables::EnvVariables,
    utils::{is_conda_env, is_conda_install},
};
use log::trace;
use pet_fs::path::{expand_path, norm_case};
use pet_python_utils::platform_dirs::Platformdirs;
use std::{
    fs,
    path::{Path, PathBuf},
    thread,
};

const APP_NAME: &str = "conda";

pub fn get_conda_environment_paths(env_vars: &EnvVariables) -> Vec<PathBuf> {
    let mut env_paths = thread::scope(|s| {
        let mut envs = vec![];
        for thread in [
            s.spawn(|| get_conda_envs_from_environment_txt(env_vars)),
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

    env_paths = env_paths.iter().map(norm_case).collect();
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
        trace!("Conda environments in .condarc {:?}", conda_rc.env_dirs);
        conda_rc.env_dirs
    } else {
        trace!("No Conda environments in .condarc");
        vec![]
    }
}

fn get_conda_environment_paths_from_known_paths(env_vars: &EnvVariables) -> Vec<PathBuf> {
    let mut env_paths: Vec<PathBuf> = vec![];
    if let Some(ref home) = env_vars.home {
        let mut known_conda_paths = vec![
            PathBuf::from(".conda/envs"),
            PathBuf::from("/opt/conda/envs"),
            PathBuf::from("C:/Anaconda/envs"),
            PathBuf::from("AppData/Local/conda/envs"),
            PathBuf::from("AppData/Local/conda/conda/envs"),
            // https://docs.conda.io/projects/conda/en/22.11.x/user-guide/configuration/use-condarc.html
            PathBuf::from("envs"),
            PathBuf::from("my-envs"),
        ]
        .into_iter()
        .map(|p| home.join(p))
        .collect::<Vec<PathBuf>>();

        // https://github.com/conda/conda/blob/d88fc157818cd5542029e116dcf4ec427512be82/conda/base/context.py#L143
        if let Some(user_data_dir) = Platformdirs::new(APP_NAME.into(), false).user_data_dir() {
            known_conda_paths.push(user_data_dir.join("envs"));
        }

        // Expland variables in some of these
        // https://docs.conda.io/projects/conda/en/4.13.x/user-guide/configuration/use-condarc.html#expansion-of-environment-variables
        if let Some(conda_envs_path) = &env_vars.conda_envs_path {
            let conda_envs_path = expand_path(PathBuf::from(conda_envs_path.clone()));
            known_conda_paths.push(conda_envs_path);
        }

        for path in known_conda_paths {
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.filter_map(Result::ok) {
                    let path = entry.path();
                    if path.is_dir() {
                        env_paths.push(path);
                    }
                }
            }
        }
    }
    env_paths.append(&mut env_vars.known_global_search_locations.clone());
    trace!("Conda environments in known paths {:?}", env_paths);
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
        // Then read the .condarc in the conda install folder as well.
        if let Some(mut conda_rc) = Condarc::from_path(conda_dir) {
            envs.append(&mut conda_rc.env_dirs);
        }
    } else if is_conda_env(conda_dir) {
        envs.push(conda_dir.to_path_buf());
    } else if conda_dir.join("envs").exists() {
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
    } else {
        // The dir could already be the `envs` directory.
        if let Ok(entries) = fs::read_dir(conda_dir) {
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
                let line = norm_case(&PathBuf::from(line.to_string()));
                trace!("Conda env in environments.txt file {:?}", line);
                envs.push(line);
            }
        }
    }

    envs
}

#[cfg(windows)]
pub fn get_known_conda_install_locations(env_vars: &EnvVariables) -> Vec<PathBuf> {
    use pet_fs::path::norm_case;

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
            known_paths.push(Path::new(&env_variable).join("micromamba"));
        }
    }
    if !home_drive.is_empty() {
        if home_drive.ends_with(':') {
            home_drive = format!("{}\\", home_drive);
        }
        known_paths.push(Path::new(&home_drive).join("anaconda3"));
        known_paths.push(Path::new(&home_drive).join("miniconda"));
        known_paths.push(Path::new(&home_drive).join("miniforge3"));
        known_paths.push(Path::new(&home_drive).join("micromamba"));
    }
    if let Some(ref conda_root) = env_vars.conda_root {
        known_paths.push(expand_path(PathBuf::from(conda_root.clone())));
    }
    if let Some(ref conda_prefix) = env_vars.conda_prefix {
        known_paths.push(expand_path(PathBuf::from(conda_prefix.clone())));
    }
    if let Some(ref conda) = env_vars.conda {
        known_paths.push(PathBuf::from(conda));
    }
    if let Some(home) = env_vars.clone().home {
        known_paths.push(home.clone().join("anaconda3"));
        known_paths.push(home.clone().join("miniconda3"));
        known_paths.push(home.clone().join("miniforge3"));
        known_paths.push(home.clone().join("micromamba"));
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
    // Ensure the casing of the paths are correct.
    // Its possible the actual path is in a different case.
    // E.g. instead of C:\username\miniconda it might bt C:\username\Miniconda
    // We use lower cases above, but it could be in any case on disc.
    // We do not want to have duplicates in different cases.
    // & we'd like to preserve the case of the original path as on disc.
    known_paths = known_paths.iter().map(norm_case).collect();
    known_paths.sort();
    known_paths.dedup();

    known_paths
}

#[cfg(unix)]
pub fn get_known_conda_install_locations(env_vars: &EnvVariables) -> Vec<PathBuf> {
    let mut known_paths = vec![];
    let home_value = env_vars.clone().home.unwrap_or_default();
    let directories_to_look_in = [
        "/opt",
        "/usr/share",
        "/usr/local",
        "/usr",
        home_value.to_str().unwrap_or_default(),
        "", // We need to look in `/anaconda3` and `/miniconda3` as well.
    ];
    for directory in directories_to_look_in.iter() {
        known_paths.push(PathBuf::from(format!("{directory}/anaconda")));
        known_paths.push(PathBuf::from(format!("{directory}/anaconda3")));
        known_paths.push(PathBuf::from(format!("{directory}/miniconda")));
        known_paths.push(PathBuf::from(format!("{directory}/miniconda3")));
        known_paths.push(PathBuf::from(format!("{directory}/miniforge")));
        known_paths.push(PathBuf::from(format!("{directory}/miniforge3")));
        known_paths.push(PathBuf::from(format!("{directory}/micromamba")));
    }
    if let Some(ref conda_root) = env_vars.conda_root {
        known_paths.push(expand_path(PathBuf::from(conda_root.clone())));
    }
    if let Some(ref conda_prefix) = env_vars.conda_prefix {
        known_paths.push(expand_path(PathBuf::from(conda_prefix.clone())));
    }
    if let Some(ref conda_dir) = env_vars.conda_dir {
        known_paths.push(expand_path(PathBuf::from(conda_dir.clone())));
    }
    if let Some(ref conda) = env_vars.conda {
        known_paths.push(expand_path(PathBuf::from(conda)));
    }
    if let Some(ref home) = env_vars.home {
        known_paths.push(home.clone().join("anaconda"));
        known_paths.push(home.clone().join("anaconda3"));
        known_paths.push(home.clone().join("miniconda"));
        known_paths.push(home.clone().join("miniconda3"));
        known_paths.push(home.clone().join("miniforge3"));
        known_paths.push(home.clone().join("micromamba"));
        known_paths.push(home.join(".conda"));
    }
    known_paths.sort();
    known_paths.dedup();

    known_paths
}
