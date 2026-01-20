// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use env_variables::EnvVariables;
use environment_locations::list_environments;
use lazy_static::lazy_static;
use log::trace;
use manager::PoetryManager;
use pet_core::{
    env::PythonEnv,
    os_environment::Environment,
    python_environment::{PythonEnvironment, PythonEnvironmentKind},
    reporter::Reporter,
    Configuration, Locator, LocatorKind, LocatorResult,
};
use pet_virtualenv::is_virtualenv;
use regex::Regex;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use telemetry::report_missing_envs;

pub mod config;
pub mod env_variables;
mod environment;
pub mod environment_locations;
mod environment_locations_spawn;
pub mod manager;
mod pyproject_toml;
mod telemetry;

lazy_static! {
    static ref POETRY_ENV_NAME_PATTERN: Regex = Regex::new(r"^.+-[A-Za-z0-9_-]{8}-py.*$")
        .expect("Error generating RegEx for poetry environment name pattern");
}

/// Check if a path looks like a Poetry environment in the cache directory
/// Poetry cache environments have names like: {name}-{hash}-py{version}
/// and are located in cache directories containing "pypoetry/virtualenvs"
fn is_poetry_cache_environment(path: &Path) -> bool {
    // Check if the environment is in a directory that looks like Poetry's virtualenvs cache
    // Common patterns:
    // - Linux: ~/.cache/pypoetry/virtualenvs/
    // - macOS: ~/Library/Caches/pypoetry/virtualenvs/
    // - Windows: %LOCALAPPDATA%\pypoetry\Cache\virtualenvs\
    let path_str = path.to_str().unwrap_or_default();

    // Check if path contains typical Poetry cache directory structure
    if path_str.contains("pypoetry") && path_str.contains("virtualenvs") {
        // Further validate by checking if the directory name matches Poetry's naming pattern
        // Pattern: {name}-{8-char-hash}-py or just .venv
        if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
            // Check for Poetry's hash-based naming: name-XXXXXXXX-py
            // The hash is 8 characters of base64url encoding
            if POETRY_ENV_NAME_PATTERN.is_match(dir_name) {
                return true;
            }
        }
    }

    false
}

/// Check if a .venv directory is an in-project Poetry environment
/// This is for the case when virtualenvs.in-project = true is set.
/// We check if the parent directory has a pyproject.toml with Poetry configuration.
fn is_in_project_poetry_environment(path: &Path) -> bool {
    // Check if this is a .venv directory
    let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if dir_name != ".venv" {
        return false;
    }

    // Check if the parent directory has a pyproject.toml with Poetry configuration
    if let Some(parent) = path.parent() {
        let pyproject_toml = parent.join("pyproject.toml");
        if pyproject_toml.is_file() {
            // Check if pyproject.toml contains Poetry configuration
            if let Ok(contents) = std::fs::read_to_string(&pyproject_toml) {
                // Look for [tool.poetry] or [project] with poetry as build backend
                if contents.contains("[tool.poetry]")
                    || (contents.contains("poetry.core.masonry.api")
                        || contents.contains("poetry-core"))
                {
                    trace!(
                        "Found in-project Poetry environment: {:?} with pyproject.toml at {:?}",
                        path,
                        pyproject_toml
                    );
                    return true;
                }
            }
        }
    }

    false
}

pub trait PoetryLocator: Send + Sync {
    fn find_and_report_missing_envs(
        &self,
        reporter: &dyn Reporter,
        poetry_executable: Option<PathBuf>,
    ) -> Option<()>;
}

pub struct Poetry {
    pub workspace_directories: Arc<Mutex<Vec<PathBuf>>>,
    pub env_vars: EnvVariables,
    pub poetry_executable: Arc<Mutex<Option<PathBuf>>>,
    search_result: Arc<Mutex<Option<LocatorResult>>>,
}

impl Poetry {
    pub fn new(environment: &dyn Environment) -> Self {
        Poetry {
            search_result: Arc::new(Mutex::new(None)),
            workspace_directories: Arc::new(Mutex::new(vec![])),
            env_vars: EnvVariables::from(environment),
            poetry_executable: Arc::new(Mutex::new(None)),
        }
    }
    fn clear(&self) {
        self.poetry_executable.lock().unwrap().take();
        self.search_result.lock().unwrap().take();
    }
    pub fn from(environment: &dyn Environment) -> Poetry {
        Poetry::new(environment)
    }
    fn find_with_cache(&self) -> Option<LocatorResult> {
        let mut search_result = self.search_result.lock().unwrap();
        if let Some(result) = search_result.clone() {
            return Some(result);
        }

        // First find the manager
        let manager = manager::PoetryManager::find(
            self.poetry_executable.lock().unwrap().clone(),
            &self.env_vars,
        );
        trace!("Poetry Manager {:?}", manager);
        let mut result = LocatorResult {
            managers: vec![],
            environments: vec![],
        };
        if let Some(manager) = &manager {
            result.managers.push(manager.to_manager());
        }

        let workspace_dirs = self.workspace_directories.lock().unwrap().clone();
        let envs = list_environments(&self.env_vars, &workspace_dirs, manager).unwrap_or_default();
        result.environments.extend(envs.clone());

        // Having a value in the search result means that we have already searched for environments
        search_result.replace(result.clone());

        if result.managers.is_empty() && result.environments.is_empty() {
            None
        } else {
            Some(result)
        }
    }
}

impl PoetryLocator for Poetry {
    fn find_and_report_missing_envs(
        &self,
        reporter: &dyn Reporter,
        poetry_executable: Option<PathBuf>,
    ) -> Option<()> {
        let user_provided_poetry_exe = poetry_executable.is_some();
        let manager = PoetryManager::find(poetry_executable.clone(), &self.env_vars)?;
        let poetry_executable = manager.executable.clone();

        let workspace_dirs = self.workspace_directories.lock().unwrap().clone();
        let environments_using_spawn = environment_locations_spawn::list_environments(
            &poetry_executable,
            &workspace_dirs,
            &manager,
        );

        let result = self.search_result.lock().unwrap().clone();
        let _ = report_missing_envs(
            reporter,
            &poetry_executable,
            workspace_dirs,
            &self.env_vars,
            &environments_using_spawn,
            result,
            user_provided_poetry_exe,
        );

        Some(())
    }
}

impl Locator for Poetry {
    fn get_kind(&self) -> LocatorKind {
        LocatorKind::Poetry
    }
    fn configure(&self, config: &Configuration) {
        if let Some(workspace_directories) = &config.workspace_directories {
            self.workspace_directories.lock().unwrap().clear();
            if !workspace_directories.is_empty() {
                self.workspace_directories
                    .lock()
                    .unwrap()
                    .extend(workspace_directories.clone());
            }
        }
        if let Some(exe) = &config.poetry_executable {
            self.poetry_executable.lock().unwrap().replace(exe.clone());
        }
    }

    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::Poetry]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if !is_virtualenv(env) {
            return None;
        }

        // First, check if the environment is in our cache
        if let Some(result) = self.find_with_cache() {
            for found_env in result.environments {
                if let Some(symlinks) = &found_env.symlinks {
                    if symlinks.contains(&env.executable) {
                        return Some(found_env.clone());
                    }
                }
            }
        }

        // Fallback: Check if the path looks like a Poetry environment
        // This handles cases where the environment wasn't discovered during find()
        // (e.g., workspace directories not configured, or pyproject.toml not found)
        if let Some(prefix) = &env.prefix {
            if is_poetry_cache_environment(prefix) {
                trace!(
                    "Identified Poetry environment by cache path pattern: {:?}",
                    prefix
                );
                return environment::create_poetry_env(
                    prefix,
                    prefix.clone(), // We don't have the project directory, use prefix
                    None,           // No manager available in this fallback case
                );
            }

            // Check for in-project .venv Poetry environment
            if is_in_project_poetry_environment(prefix) {
                trace!(
                    "Identified in-project Poetry environment: {:?}",
                    prefix
                );
                // For in-project .venv, the project directory is the parent
                let project_dir = prefix.parent().unwrap_or(prefix).to_path_buf();
                return environment::create_poetry_env(
                    prefix,
                    project_dir,
                    None, // No manager available in this fallback case
                );
            }
        }

        None
    }

    fn find(&self, reporter: &dyn Reporter) {
        self.clear();
        if let Some(result) = self.find_with_cache() {
            for manager in result.managers {
                reporter.report_manager(&manager.clone());
            }
            for found_env in result.environments {
                reporter.report_environment(&found_env);
            }
        }
    }
}
