// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::{env_variables::EnvVariables, environment_locations::get_work_on_home_path};
use pet_utils::{
    env::PythonEnv, executable::find_executable, path::normalize, pyvenv_cfg::PyVenvCfg,
};
use pet_virtualenv::is_virtualenv;
use std::{fs, path::PathBuf};

pub fn is_virtualenvwrapper(env: &PythonEnv, environment: &EnvVariables) -> bool {
    if env.prefix.is_none() {
        return false;
    }

    // For environment to be a virtualenvwrapper based it has to follow these two rules:
    // 1. It should be in a sub-directory under the WORKON_HOME
    // 2. It should be a valid virtualenv environment
    if let Some(work_on_home_dir) = get_work_on_home_path(environment) {
        if env.executable.starts_with(work_on_home_dir) && is_virtualenv(env) {
            return true;
        }
    }

    false
}

pub fn get_project(env: &PythonEnv) -> Option<PathBuf> {
    let project_file = env.prefix.clone()?.join(".project");
    let contents = fs::read_to_string(&project_file).ok()?;
    println!("contents of project file {:?}: {:?}", project_file, contents);
    let project_folder = normalize(PathBuf::from(contents.trim().to_string()));
    if fs::metadata(&project_folder).is_ok() {
        Some(normalize(&project_folder))
    } else {
        None
    }
}

pub fn list_python_environments(path: &PathBuf) -> Option<Vec<PythonEnv>> {
    let mut python_envs: Vec<PythonEnv> = vec![];
    for venv_dir in fs::read_dir(path)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
    {
        if fs::metadata(&venv_dir).is_err() {
            continue;
        }
        if let Some(executable) = find_executable(&venv_dir) {
            python_envs.push(PythonEnv::new(
                executable.clone(),
                Some(venv_dir.clone()),
                PyVenvCfg::find(&venv_dir).map(|cfg| cfg.version),
            ));
        }
    }

    Some(python_envs)
}
