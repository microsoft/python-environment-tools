// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use env_logger::Builder;
use log::{trace, LevelFilter};
use pet_core::{
    manager::EnvManager,
    python_environment::PythonEnvironment,
    reporter::Reporter,
    telemetry::{get_telemetry_event_name, TelemetryEvent},
};
use pet_jsonrpc::send_message;
use serde::{Deserialize, Serialize};

pub struct JsonRpcReporter {}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Debug)]
struct TelemetryData {
    event: String,
    data: TelemetryEvent,
}

impl Reporter for JsonRpcReporter {
    fn report_telemetry(&self, event: &TelemetryEvent) {
        let event = TelemetryData {
            event: get_telemetry_event_name(event).to_string(),
            data: event.clone(),
        };
        trace!("Telemetry event {:?}", event.event);
        send_message("telemetry", Some(event))
    }
    fn report_manager(&self, manager: &EnvManager) {
        trace!("Reporting Manager {:?}", manager);
        send_message("manager", manager.into())
    }

    fn report_environment(&self, env: &PythonEnvironment) {
        trace!("Reporting Environment {:?}", env);
        send_message("environment", env.into())
    }
}

pub fn create_reporter() -> impl Reporter {
    JsonRpcReporter {}
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
