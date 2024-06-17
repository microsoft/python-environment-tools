// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use inaccurate_python_info::InAccuratePythonEnvironmentInfo;

pub mod inaccurate_python_info;

pub type NumberOfCustomSearchPaths = u32;

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
    InaccuratePythonEnvironmentInfo(InAccuratePythonEnvironmentInfo),
}
