// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use env_variables::EnvVariables;
use environment_locations::list_environments;
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
use std::{
    path::PathBuf,
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
        if let Some(result) = self.find_with_cache() {
            for found_env in result.environments {
                if let Some(symlinks) = &found_env.symlinks {
                    if symlinks.contains(&env.executable) {
                        return Some(found_env.clone());
                    }
                }
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
