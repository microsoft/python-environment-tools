// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    fs,
    path::{Path, PathBuf},
};

use log::{error, trace};
use pet_python_utils::platform_dirs::Platformdirs;

use crate::env_variables::EnvVariables;

static _APP_NAME: &str = "pypoetry";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub virtualenvs_in_project: Option<bool>,
    pub virtualenvs_path: PathBuf,
    pub file: Option<PathBuf>,
}

impl Config {
    fn new(
        file: Option<PathBuf>,
        virtualenvs_path: PathBuf,
        virtualenvs_in_project: Option<bool>,
    ) -> Self {
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
        let file = find_config_file(env);
        create_config(file, env)
    }
    pub fn find_local(path: &Path, env: &EnvVariables) -> Option<Self> {
        let file = path.join("poetry.toml");
        if file.is_file() {
            create_config(Some(file), env)
        } else {
            None
        }
    }
}

fn create_config(file: Option<PathBuf>, env: &EnvVariables) -> Option<Config> {
    let cfg = file.clone().and_then(|f| parse(&f));
    if let Some(virtualenvs_path) = &cfg.clone().and_then(|cfg| cfg.virtualenvs_path) {
        return Some(Config::new(
            file.clone(),
            virtualenvs_path.clone(),
            cfg.and_then(|cfg| cfg.virtualenvs_in_project),
        ));
    }

    get_default_cache_dir(env)
        .map(|cache_dir| Config::new(file, cache_dir.join("virtualenvs"), None))
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
        // Ensure we have a valid directory setup in the env variables.
        if config.is_dir() {
            return Some(config);
        }
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfigToml {
    virtualenvs_in_project: Option<bool>,
    cache_dir: Option<PathBuf>,
    virtualenvs_path: Option<PathBuf>,
}

fn parse(file: &Path) -> Option<ConfigToml> {
    let contents = fs::read_to_string(file).ok()?;
    parse_contents(&contents)
}

fn parse_contents(contents: &str) -> Option<ConfigToml> {
    let mut virtualenvs_path = None;
    let mut cache_dir = None;
    let mut virtualenvs_in_project = None;
    match toml::from_str::<toml::Value>(contents) {
        Ok(value) => {
            if let Some(virtualenvs) = value.get("virtualenvs") {
                if let Some(path) = virtualenvs.get("path") {
                    // Can contain invalid toml, hence make no assumptions
                    // virtualenvs.in-project = null
                    // https://github.com/python-poetry/poetry/blob/5bab98c9500f1050c6bb6adfb55580a23173f18d/docs/configuration.md#L56
                    if path.is_str() {
                        virtualenvs_path = path.as_str().map(|s| s.trim()).map(PathBuf::from);
                    }
                }
                if let Some(in_project) = virtualenvs.get("in-project") {
                    // Can contain invalid toml, hence make no assumptions
                    // virtualenvs.in-project = null
                    // https://github.com/python-poetry/poetry/blob/5bab98c9500f1050c6bb6adfb55580a23173f18d/docs/configuration.md#L56
                    if in_project.is_bool() {
                        virtualenvs_in_project = in_project.as_bool();
                    }
                }
            }
            if let Some(value) = value.get("cache-dir") {
                // Can contain invalid toml, hence make no assumptions
                // virtualenvs.in-project = null
                // https://github.com/python-poetry/poetry/blob/5bab98c9500f1050c6bb6adfb55580a23173f18d/docs/configuration.md#L56
                if value.is_str() {
                    cache_dir = value.as_str().map(|s| s.trim()).map(PathBuf::from);

                    if let Some(cache_dir) = &cache_dir {
                        if virtualenvs_path.is_none() {
                            virtualenvs_path = Some(cache_dir.join("virtualenvs"));
                        }
                    }
                }
            }

            Some(ConfigToml {
                virtualenvs_in_project,
                virtualenvs_path,
                cache_dir,
            })
        }
        Err(e) => {
            error!("Error parsing poetry toml file: {:?}", e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_virtualenvs_in_poetry_toml() {
        let cfg = r#"
[virtualenvs]
in-project = false
create = false

"#;

        assert_eq!(
            parse_contents(&cfg.to_string())
                .unwrap()
                .virtualenvs_in_project
                .unwrap_or_default(),
            false
        );

        let cfg = r#"
[virtualenvs]
in-project = true
create = false

"#;
        assert_eq!(
            parse_contents(&cfg.to_string())
                .unwrap()
                .virtualenvs_in_project
                .unwrap_or_default(),
            true
        );

        let cfg = r#"
[virtualenvs]
create = false

"#;
        assert_eq!(
            parse_contents(&cfg.to_string())
                .unwrap()
                .virtualenvs_in_project
                .unwrap_or_default(),
            false
        );

        let cfg = r#"
virtualenvs.in-project = true # comment
"#;
        assert_eq!(
            parse_contents(&cfg.to_string())
                .unwrap()
                .virtualenvs_in_project
                .unwrap_or_default(),
            true
        );

        let cfg = r#"
"#;
        assert_eq!(
            parse_contents(&cfg.to_string())
                .unwrap()
                .virtualenvs_in_project
                .unwrap_or_default(),
            false
        );
    }

    #[test]
    fn parse_cache_dir_in_poetry_toml() {
        let cfg = r#"
cache-dir = "/path/to/cache/directory"

"#;
        assert_eq!(
            parse_contents(&cfg.to_string()).unwrap().cache_dir,
            Some(PathBuf::from("/path/to/cache/directory".to_string()))
        );

        let cfg = r#"
some-other-value = 1234

"#;
        assert_eq!(parse_contents(&cfg.to_string()).unwrap().cache_dir, None);
    }

    #[test]
    fn parse_virtualenvs_path_in_poetry_toml() {
        let cfg = r#"
virtualenvs.path = "/path/to/virtualenvs"

"#;
        assert_eq!(
            parse_contents(&cfg.to_string()).unwrap().virtualenvs_path,
            Some(PathBuf::from("/path/to/virtualenvs".to_string()))
        );

        let cfg = r#"
some-other-value = 1234

"#;
        assert_eq!(
            parse_contents(&cfg.to_string()).unwrap().virtualenvs_path,
            None
        );
    }

    #[test]
    fn use_cache_dir_to_build_virtualenvs_path() {
        let cfg = r#"
cache-dir = "/path/to/cache/directory"
"#;
        assert_eq!(
            parse_contents(&cfg.to_string()).unwrap().virtualenvs_path,
            Some(PathBuf::from("/path/to/cache/directory/virtualenvs"))
        );
    }
}
