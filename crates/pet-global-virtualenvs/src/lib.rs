// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_conda::utils::is_conda_env;
use pet_utils::path::normalize;
use std::{fs, path::PathBuf};

fn get_global_virtualenv_dirs(
    work_on_home_env_var: Option<String>,
    user_home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut venv_dirs: Vec<PathBuf> = vec![];

    if let Some(work_on_home) = work_on_home_env_var {
        let work_on_home = normalize(PathBuf::from(work_on_home));
        if fs::metadata(&work_on_home).is_ok() {
            venv_dirs.push(work_on_home);
        }
    }

    if let Some(home) = user_home {
        for dir in [
            PathBuf::from("envs"),
            PathBuf::from(".direnv"),
            PathBuf::from(".venvs"),
            PathBuf::from(".virtualenvs"),
            PathBuf::from(".local").join("share").join("virtualenvs"),
        ] {
            let venv_dir = home.join(dir);
            if fs::metadata(&venv_dir).is_ok() {
                venv_dirs.push(venv_dir);
            }
        }
        if cfg!(target_os = "linux") {
            let envs = PathBuf::from("Envs");
            if fs::metadata(&envs).is_ok() {
                venv_dirs.push(envs);
            }
        }
    }

    venv_dirs
}

pub fn list_global_virtual_envs_paths(
    work_on_home_env_var: Option<String>,
    user_home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut python_envs: Vec<PathBuf> = vec![];
    for root_dir in &get_global_virtualenv_dirs(work_on_home_env_var, user_home) {
        if let Ok(dirs) = fs::read_dir(root_dir) {
            python_envs.append(
                &mut dirs
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| !is_conda_env(p))
                    .collect(),
            )
        }
    }

    python_envs.sort();
    python_envs.dedup();

    python_envs
}
