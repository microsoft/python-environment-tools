// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use env_variables::EnvVariables;
use environment_locations::list_environments;
use log::{error, warn};
use pet_core::{
    os_environment::Environment,
    python_environment::{PythonEnvironment, PythonEnvironmentCategory},
    reporter::Reporter,
    Configuration, Locator, LocatorResult,
};
use pet_python_utils::env::PythonEnv;
use pet_virtualenv::is_virtualenv;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

pub mod config;
pub mod env_variables;
mod environment;
pub mod environment_locations;
mod environment_locations_spawn;
pub mod manager;
mod pyproject_toml;

pub struct Poetry {
    pub project_dirs: Arc<Mutex<Vec<PathBuf>>>,
    pub env_vars: EnvVariables,
    pub poetry_executable: Arc<Mutex<Option<PathBuf>>>,
    searched: AtomicBool,
    search_result: Arc<Mutex<Option<LocatorResult>>>,
}

impl Poetry {
    pub fn new(environment: &dyn Environment) -> Self {
        Poetry {
            searched: AtomicBool::new(false),
            search_result: Arc::new(Mutex::new(None)),
            project_dirs: Arc::new(Mutex::new(vec![])),
            env_vars: EnvVariables::from(environment),
            poetry_executable: Arc::new(Mutex::new(None)),
        }
    }
    pub fn from(environment: &dyn Environment) -> impl Locator {
        Poetry::new(environment)
    }
    pub fn find_with_executable(&self) -> Option<()> {
        let manager = manager::PoetryManager::find(
            self.poetry_executable.lock().unwrap().clone(),
            &self.env_vars,
        )?;

        let environments_using_spawn = environment_locations_spawn::list_environments(
            &manager.executable,
            self.project_dirs.lock().unwrap().clone(),
            &manager,
        )
        .iter()
        .filter_map(|env| env.prefix.clone())
        .collect::<Vec<_>>();

        // Get environments using the faster way.
        if let Some(environments) = &self.find_with_cache() {
            let environments = environments
                .environments
                .iter()
                .filter_map(|env| env.prefix.clone())
                .collect::<Vec<_>>();

            for env in environments_using_spawn {
                if !environments.contains(&env) {
                    warn!(
                        "Found a Poetry env {:?} using the poetry exe {:?}",
                        env, manager.executable
                    );
                    // TODO: Send telemetry.
                }
            }
        } else {
            // TODO: Send telemetry.
            for env in environments_using_spawn {
                warn!(
                    "Found a Poetry env {:?} using the poetry exe {:?}",
                    env, manager.executable
                );
            }
        }
        Some(())
    }
    fn find_with_cache(&self) -> Option<LocatorResult> {
        if self.searched.load(Ordering::Relaxed) {
            return self.search_result.lock().unwrap().clone();
        }
        // First find the manager
        let manager = manager::PoetryManager::find(
            self.poetry_executable.lock().unwrap().clone(),
            &self.env_vars,
        );
        let mut result = LocatorResult {
            managers: vec![],
            environments: vec![],
        };
        if let Some(manager) = manager {
            result.managers.push(manager.to_manager());
        }
        if let Ok(values) = self.project_dirs.lock() {
            let project_dirs = values.clone();
            drop(values);
            let envs = list_environments(&self.env_vars, &project_dirs.clone()).unwrap_or_default();
            result.environments.extend(envs.clone());
        }

        match self.search_result.lock().as_mut() {
            Ok(search_result) => {
                if result.managers.is_empty() && result.environments.is_empty() {
                    search_result.take();
                    None
                } else {
                    search_result.replace(result.clone());
                    Some(result)
                }
            }
            Err(err) => {
                error!("Failed to cache to Poetry environments: {:?}", err);
                None
            }
        }
    }
}

impl Locator for Poetry {
    fn configure(&self, config: &Configuration) {
        if let Some(search_paths) = &config.search_paths {
            if !search_paths.is_empty() {
                self.project_dirs.lock().unwrap().clear();
                self.project_dirs
                    .lock()
                    .unwrap()
                    .extend(search_paths.clone());
            }
        }
        if let Some(exe) = &config.poetry_executable {
            self.poetry_executable.lock().unwrap().replace(exe.clone());
        }
    }

    fn supported_categories(&self) -> Vec<PythonEnvironmentCategory> {
        vec![PythonEnvironmentCategory::Poetry]
    }

    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
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
