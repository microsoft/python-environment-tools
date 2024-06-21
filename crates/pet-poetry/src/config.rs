// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    fs,
    path::{Path, PathBuf},
};

use log::trace;
use pet_python_utils::platform_dirs::Platformdirs;

use crate::env_variables::EnvVariables;

static _APP_NAME: &str = "pypoetry";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub virtualenvs_in_project: bool,
    pub virtualenvs_path: PathBuf,
    pub file: PathBuf,
}

impl Config {
    fn new(file: PathBuf, virtualenvs_path: PathBuf, virtualenvs_in_project: bool) -> Self {
        trace!(
            "Poetry config file: {:?} with virtualenv.path {:?}",
            file,
            virtualenvs_path
        );
        Config {
            file,
            virtualenvs_path,
            virtualenvs_in_project,
        }
    }
    pub fn find_global(env: &EnvVariables) -> Option<Self> {
        let file = find_config_file(env)?;
        create_config(&file, env)
    }
    pub fn find_local(path: &Path, env: &EnvVariables) -> Option<Self> {
        let file = path.join("poetry.toml");
        if file.is_file() {
            create_config(&file, env)
        } else {
            None
        }
    }
}

fn create_config(file: &Path, env: &EnvVariables) -> Option<Config> {
    let cfg = parse(file)?;

    if let Some(virtualenvs_path) = &cfg.virtualenvs_path {
        return Some(Config::new(
            file.to_path_buf(),
            virtualenvs_path.clone(),
            cfg.virtualenvs_in_project,
        ));
    }

    let cache_dir = match cfg.cache_dir {
        Some(cache_dir) => {
            if cache_dir.is_dir() {
                Some(cache_dir)
            } else {
                get_default_cache_dir(env)
            }
        }
        None => get_default_cache_dir(env),
    };

    if let Some(cache_dir) = cache_dir {
        Some(Config::new(
            file.to_path_buf(),
            cache_dir.join("virtualenvs"),
            cfg.virtualenvs_in_project,
        ))
    } else {
        None
    }
}
/// Maps to DEFAULT_CACHE_DIR in poetry
fn get_default_cache_dir(env: &EnvVariables) -> Option<PathBuf> {
    if let Some(cache_dir) = env.poetry_cache_dir.clone() {
        Some(cache_dir)
    } else {
        Platformdirs::new(_APP_NAME.into(), false).user_cache_path()
    }
}

/// Maps to CONFIG_DIR in poetry
fn get_config_dir(env: &EnvVariables) -> Option<PathBuf> {
    if let Some(config) = env.poetry_config_dir.clone() {
        return Some(config);
    }
    Platformdirs::new(_APP_NAME.into(), true).user_config_path()
}

fn find_config_file(env: &EnvVariables) -> Option<PathBuf> {
    let config_dir = get_config_dir(env)?;
    let file = config_dir.join("config.toml");
    if file.exists() {
        Some(file)
    } else {
        None
    }
}

struct ConfigToml {
    virtualenvs_in_project: bool,
    cache_dir: Option<PathBuf>,
    virtualenvs_path: Option<PathBuf>,
}

fn parse(file: &Path) -> Option<ConfigToml> {
    let contents = fs::read_to_string(file).ok()?;

    let mut virtualenvs_path = None;
    let mut cache_dir = None;
    let mut virtualenvs_in_project = false;
    match toml::from_str::<toml::Value>(&contents) {
        Ok(value) => {
            if let Some(virtualenvs) = value.get("virtualenvs") {
                if let Some(path) = virtualenvs.get("path") {
                    virtualenvs_path = path.as_str().map(|s| s.trim()).map(PathBuf::from);
                }
                if let Some(in_project) = virtualenvs.get("in-project") {
                    virtualenvs_in_project = in_project.as_bool().unwrap_or_default();
                }
            }
            if let Some(value) = value.get("cache-dir") {
                cache_dir = value.as_str().map(|s| s.trim()).map(PathBuf::from);
            }

            Some(ConfigToml {
                virtualenvs_in_project,
                virtualenvs_path,
                cache_dir,
            })
        }
        Err(e) => {
            eprintln!("Error parsing toml file: {:?}", e);
            None
        }
    }
}
