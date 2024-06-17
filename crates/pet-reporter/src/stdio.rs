// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::{
    environment::{get_environment_key, Environment},
    manager::Manager,
};
use env_logger::Builder;
use log::LevelFilter;
use pet_core::{manager::EnvManager, python_environment::PythonEnvironment, reporter::Reporter};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub struct StdioReporter {
    reported_managers: Arc<Mutex<HashSet<PathBuf>>>,
    reported_environments: Arc<Mutex<HashSet<PathBuf>>>,
}

impl Reporter for StdioReporter {
    fn report_manager(&self, manager: &EnvManager) {
        let mut reported_managers = self.reported_managers.lock().unwrap();
        if !reported_managers.contains(&manager.executable) {
            reported_managers.insert(manager.executable.clone());
            let prefix = format!("{}.", reported_managers.len());
            println!("{:<3}{}", prefix, Manager::from(manager))
        }
    }

    fn report_environment(&self, env: &PythonEnvironment) {
        if let Some(key) = get_environment_key(env) {
            let mut reported_environments = self.reported_environments.lock().unwrap();
            if !reported_environments.contains(&key) {
                reported_environments.insert(key.clone());
                let prefix = format!("{}.", reported_environments.len());
                println!("{:<3}{}", prefix, Environment::from(env))
            }
        }
    }
}

pub fn create_reporter() -> impl Reporter {
    StdioReporter {
        reported_managers: Arc::new(Mutex::new(HashSet::new())),
        reported_environments: Arc::new(Mutex::new(HashSet::new())),
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Eq, Clone)]
pub enum LogLevel {
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "error")]
    Error,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Log {
    pub message: String,
    pub level: LogLevel,
}

pub fn initialize_logger(log_level: LevelFilter) {
    Builder::new().filter(None, log_level).init();
}
