// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use base64::{engine::general_purpose, Engine as _};
use lazy_static::lazy_static;
use log::trace;
use pet_core::python_environment::PythonEnvironment;
use pet_fs::path::norm_case;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    config::Config, env_variables::EnvVariables, environment::create_poetry_env,
    pyproject_toml::PyProjectToml,
};

lazy_static! {
    static ref SANITIZE_NAME: Regex = Regex::new("[ $`!*@\"\\\r\n\t]")
        .expect("Error generating RegEx for poetry file path hash generator");
}

pub fn list_environments(
    env: &EnvVariables,
    project_dirs: &[PathBuf],
) -> Option<Vec<PythonEnvironment>> {
    if project_dirs.is_empty() {
        return None;
    }

    let project_dirs = project_dirs
        .iter()
        .map(|project_dir| (project_dir, PyProjectToml::find(project_dir)))
        .filter_map(|(project_dir, pyproject_toml)| {
            pyproject_toml.map(|pyproject_toml| (project_dir, pyproject_toml))
        })
        .collect::<Vec<_>>();

    // We're only interested in directories that have a pyproject.toml
    if project_dirs.is_empty() {
        return None;
    }

    let mut envs = vec![];

    let global_config = Config::find_global(env);
    let mut global_envs = vec![];
    if let Some(config) = global_config.clone() {
        global_envs = list_all_environments_from_config(&config).unwrap_or_default();
    }

    for (project_dir, pyproject_toml) in project_dirs {
        let virtualenv_prefix = generate_env_name(&pyproject_toml.name, project_dir);
        trace!(
            "Found pyproject.toml ({}): {:?} in {:?}",
            virtualenv_prefix,
            pyproject_toml.name,
            project_dir
        );

        for virtual_env in [
            list_all_environments_from_project_config(&global_config, project_dir, env)
                .unwrap_or_default(),
            global_envs.clone(),
        ]
        .concat()
        {
            // Check if this virtual env belongs to this project
            let name = virtual_env
                .file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default();
            if name.starts_with(&virtualenv_prefix) {
                if let Some(env) = create_poetry_env(&virtual_env, project_dir.clone(), None) {
                    envs.push(env);
                }
            }
        }
    }

    Some(envs)
}

fn list_all_environments_from_project_config(
    global: &Option<Config>,
    path: &Path,
    env: &EnvVariables,
) -> Option<Vec<PathBuf>> {
    let local = Config::find_local(path, env);
    let mut envs = vec![];

    if let Some(local) = &local {
        if let Some(project_envs) = list_all_environments_from_config(local) {
            envs.extend(project_envs);
        }
    }

    // Check if we're allowed to use .venv as a poetry env
    // This can be configured in global, project or env variable.
    // Order of preference is Global, EnvVariable & Project (project wins)
    if should_use_local_venv_as_poetry_env(global, &local, env) {
        // If virtualenvs are in the project, then look for .venv
        let venv = path.join(".venv");
        if venv.is_dir() {
            envs.push(venv);
        }
    }
    Some(envs)
}

fn should_use_local_venv_as_poetry_env(
    global: &Option<Config>,
    local: &Option<Config>,
    env: &EnvVariables,
) -> bool {
    // Give preference to setting in local config file.
    if let Some(poetry_virtualenvs_in_project) =
        local.clone().and_then(|c| c.virtualenvs_in_project)
    {
        return poetry_virtualenvs_in_project;
    }

    // Given preference to env variable.
    if let Some(poetry_virtualenvs_in_project) = env.poetry_virtualenvs_in_project {
        return poetry_virtualenvs_in_project;
    }

    // Check global config setting.
    global
        .clone()
        .and_then(|config| config.virtualenvs_in_project)
        .unwrap_or_default()
}

fn list_all_environments_from_config(cfg: &Config) -> Option<Vec<PathBuf>> {
    Some(
        fs::read_dir(&cfg.virtualenvs_path)
            .ok()?
            .filter_map(Result::ok)
            // .filter(|d| d.file_type().is_ok_and(|f| f.is_dir()))
            .map(|entry| entry.path())
            .collect(),
    )
}

// Source from https://github.com/python-poetry/poetry/blob/5bab98c9500f1050c6bb6adfb55580a23173f18d/src/poetry/utils/env/env_manager.py#L752C1-L757C63
pub fn generate_env_name(name: &str, cwd: &PathBuf) -> String {
    // name = name.lower()
    // sanitized_name = re.sub(r'[ $`!*@"\\\r\n\t]', "_", name)[:42]
    // normalized_cwd = os.path.normcase(os.path.realpath(cwd))
    // h_bytes = hashlib.sha256(encode(normalized_cwd)).digest()
    // h_str = base64.urlsafe_b64encode(h_bytes).decode()[:8]
    let sanitized_name = SANITIZE_NAME
        .replace_all(&name.to_lowercase(), "_")
        .chars()
        .take(42)
        .collect::<String>();
    let normalized_cwd = if cfg!(windows) {
        norm_case(cwd).to_str().unwrap_or_default().to_lowercase()
    } else {
        norm_case(cwd).to_str().unwrap_or_default().to_string()
    };
    let mut hasher = Sha256::new();
    hasher.update(normalized_cwd.as_bytes());
    let h_bytes = hasher.finalize();
    let h_str = general_purpose::URL_SAFE
        .encode(h_bytes)
        .chars()
        .take(8)
        .collect::<String>();
    format!("{}-{}-py", sanitized_name, h_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn test_hash_generation() {
        let hashed_name = generate_env_name(
            "poetry-demo",
            &"/Users/donjayamanne/temp/poetry-sample1/poetry-demo".into(),
        );

        assert_eq!(hashed_name, "poetry-demo-gNT2WXAV-py");
    }

    #[test]
    #[cfg(unix)]
    fn test_hash_generation_upper_case() {
        let hashed_name = generate_env_name(
            "new-project",
            &"/Users/donjayamanne/temp/POETRY-UPPER/new-PROJECT".into(),
        );

        assert_eq!(hashed_name, "new-project-TbBV0MKD-py");
    }

    #[test]
    #[cfg(windows)]
    fn test_hash_generation_windows() {
        let hashed_name = generate_env_name(
            "demo-project1",
            &"C:\\temp\\poetry-folders\\demo-project1".into(),
        );

        assert_eq!(hashed_name, "demo-project1-f7sQRtG5-py");
    }
}
