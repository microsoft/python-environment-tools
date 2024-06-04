// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::{environment::Environment, manager::Manager};
use env_logger::Builder;
use log::{error, LevelFilter};
use pet_core::{manager::EnvManager, python_environment::PythonEnvironment, reporter::Reporter};
use pet_jsonrpc::send_message;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub struct JsonRpcReporter {
    reported_managers: Arc<Mutex<HashSet<PathBuf>>>,
    reported_environments: Arc<Mutex<HashSet<PathBuf>>>,
}

impl Reporter for JsonRpcReporter {
    fn report_manager(&self, manager: &EnvManager) {
        let mut reported_managers = self.reported_managers.lock().unwrap();
        if !reported_managers.contains(&manager.executable) {
            reported_managers.insert(manager.executable.clone());
            send_message("manager", Manager::from(manager).into())
        }
    }

    fn report_environment(&self, env: &PythonEnvironment) {
        if let Some(key) = get_environment_key(env) {
            let mut reported_environments = self.reported_environments.lock().unwrap();
            if !reported_environments.contains(key) {
                reported_environments.insert(key.clone());
                send_message("environment", Environment::from(env).into())
            }
        }
    }
}

pub fn create_reporter() -> impl Reporter {
    JsonRpcReporter {
        reported_managers: Arc::new(Mutex::new(HashSet::new())),
        reported_environments: Arc::new(Mutex::new(HashSet::new())),
    }
}

fn get_environment_key(env: &PythonEnvironment) -> Option<&PathBuf> {
    if let Some(exe) = &env.executable {
        Some(exe)
    } else if let Some(prefix) = &env.prefix {
        Some(prefix)
    } else {
        error!(
            "Failed to report environment due to lack of exe & prefix: {:?}",
            env
        );
        None
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
    Builder::new()
        .format(|_, record| {
            let level = match record.level() {
                log::Level::Debug => LogLevel::Debug,
                log::Level::Error => LogLevel::Error,
                log::Level::Info => LogLevel::Info,
                log::Level::Warn => LogLevel::Warning,
                _ => LogLevel::Debug,
            };
            let payload = Log {
                message: format!("{}", record.args()).to_string(),
                level,
            };
            send_message("log", payload.into());
            Ok(())
        })
        .filter(None, log_level)
        .init();
}
