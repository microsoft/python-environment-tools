// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_conda::utils::is_conda_env;
use pet_fs::path::{expand_path, norm_case};
use std::{fs, path::PathBuf};

fn get_global_virtualenv_dirs(
    work_on_home_env_var: Option<String>,
    xdg_data_home: Option<String>,
    user_home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut venv_dirs: Vec<PathBuf> = vec![];

    if let Some(work_on_home) = work_on_home_env_var {
        let work_on_home = norm_case(expand_path(PathBuf::from(work_on_home)));
        if work_on_home.exists() {
            venv_dirs.push(work_on_home);
        }
    }

    // Used by pipenv (https://github.com/pypa/pipenv/blob/main/pipenv/utils/shell.py#L184)
    if let Some(xdg_data_home) = xdg_data_home.map(|d| PathBuf::from(d).join("virtualenvs")) {
        if xdg_data_home.exists() {
            venv_dirs.push(xdg_data_home);
        }
    }
    if let Some(home) = user_home {
        for dir in [
            PathBuf::from("envs"),
            PathBuf::from(".direnv"),
            PathBuf::from(".venvs"), // Used by pipenv, https://pipenv.pypa.io/en/latest/virtualenv.html
            PathBuf::from(".virtualenvs"), // Used by pipenv (https://github.com/pypa/pipenv/blob/main/pipenv/utils/shell.py#L184), and also default location for virtualenvwrapper, https://virtualenvwrapper.readthedocs.io/en/latest/install.html#location-of-environments
            PathBuf::from(".local").join("share").join("virtualenvs"), // Used by pipenv (https://github.com/pypa/pipenv/blob/main/pipenv/utils/shell.py#L184)
        ] {
            let venv_dir = home.join(dir);
            if venv_dir.exists() {
                venv_dirs.push(venv_dir);
            }
        }
        if cfg!(target_os = "linux") {
            // https://virtualenvwrapper.readthedocs.io/en/latest/index.html
            // Default recommended location for virtualenvwrapper
            let envs = PathBuf::from("Envs");
            if envs.exists() {
                venv_dirs.push(envs);
            }
            let envs = PathBuf::from("envs");
            if envs.exists() {
                venv_dirs.push(envs);
            }
        }
    }

    venv_dirs
}

pub fn list_global_virtual_envs_paths(
    work_on_home_env_var: Option<String>,
    xdg_data_home: Option<String>,
    user_home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut python_envs: Vec<PathBuf> = vec![];
    for root_dir in &get_global_virtualenv_dirs(work_on_home_env_var, xdg_data_home, user_home) {
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
