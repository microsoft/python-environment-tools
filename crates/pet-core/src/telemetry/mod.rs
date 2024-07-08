// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use inaccurate_python_info::InaccuratePythonEnvironmentInfo;
use missing_conda_info::MissingCondaEnvironments;
use serde::{Deserialize, Serialize};

pub mod inaccurate_python_info;
pub mod missing_conda_info;

pub type NumberOfCustomSearchPaths = u32;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Debug, Clone, Copy)]
pub enum TelemetryEvent {
    /// Total time taken to search for Global environments.
    GlobalEnvironmentsSearchCompleted(std::time::Duration),
    /// Total time taken to search for Global Virtual environments.
    GlobalVirtualEnvironmentsSearchCompleted(std::time::Duration),
    /// Total time taken to search for environments in the PATH environment variable.
    GlobalPathVariableEnvironmentsSearchCompleted(std::time::Duration),
    /// Total time taken to search for environments in specific paths provided by the user.
    /// This generally maps to workspace folders in Python extension.
    AllSearchPathsEnvironmentsSearchCompleted(std::time::Duration, NumberOfCustomSearchPaths),
    /// Total time taken to search for all environments in all locations.
    /// This is the max of all of the other `SearchCompleted` durations.
    SearchCompleted(std::time::Duration),
    /// Sent when an the information for an environment discovered is not accurate.
    InaccuratePythonEnvironmentInfo(InaccuratePythonEnvironmentInfo),
    /// Sent when an environment is discovered by spawning conda and not found otherwise.
    MissingCondaEnvironments(MissingCondaEnvironments),
}

pub fn get_telemetry_event_name(event: &TelemetryEvent) -> &'static str {
    match event {
        TelemetryEvent::GlobalEnvironmentsSearchCompleted(_) => "GlobalEnvironmentsSearchCompleted",
        TelemetryEvent::GlobalVirtualEnvironmentsSearchCompleted(_) => {
            "GlobalVirtualEnvironmentsSearchCompleted"
        }
        TelemetryEvent::GlobalPathVariableEnvironmentsSearchCompleted(_) => {
            "GlobalPathVariableEnvironmentsSearchCompleted"
        }
        TelemetryEvent::AllSearchPathsEnvironmentsSearchCompleted(_, _) => {
            "AllSearchPathsEnvironmentsSearchCompleted"
        }
        TelemetryEvent::SearchCompleted(_) => "SearchCompleted",
        TelemetryEvent::InaccuratePythonEnvironmentInfo(_) => "InaccuratePythonEnvironmentInfo",
        TelemetryEvent::MissingCondaEnvironments(_) => "MissingCondaEnvironments",
    }
}
