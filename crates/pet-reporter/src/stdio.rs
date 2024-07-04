// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use env_logger::Builder;
use log::LevelFilter;
use pet_core::{
    manager::{EnvManager, EnvManagerType},
    python_environment::{PythonEnvironment, PythonEnvironmentKind},
    reporter::Reporter,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub struct StdioReporter {
    print_list: bool,
    managers: Arc<Mutex<HashMap<EnvManagerType, u16>>>,
    environments: Arc<Mutex<HashMap<Option<PythonEnvironmentKind>, u16>>>,
}

pub struct Summary {
    pub managers: HashMap<EnvManagerType, u16>,
    pub environments: HashMap<Option<PythonEnvironmentKind>, u16>,
}

impl StdioReporter {
    pub fn get_summary(&self) -> Summary {
        let managers = self.managers.lock().unwrap();
        let environments = self.environments.lock().unwrap();
        Summary {
            managers: managers.clone(),
            environments: environments.clone(),
        }
    }
}
impl Reporter for StdioReporter {
    fn report_manager(&self, manager: &EnvManager) {
        let mut managers = self.managers.lock().unwrap();
        let count = managers.get(&manager.tool).unwrap_or(&0) + 1;
        managers.insert(manager.tool, count);
        if self.print_list {
            println!("{manager}")
        }
    }

    fn report_environment(&self, env: &PythonEnvironment) {
        let mut environments = self.environments.lock().unwrap();
        let count = environments.get(&env.kind).unwrap_or(&0) + 1;
        environments.insert(env.kind, count);
        if self.print_list {
            println!("{env}")
        }
    }
}

pub fn create_reporter(print_list: bool) -> StdioReporter {
    StdioReporter {
        print_list,
        managers: Arc::new(Mutex::new(HashMap::new())),
        environments: Arc::new(Mutex::new(HashMap::new())),
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
