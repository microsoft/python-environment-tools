// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

#[cfg(windows)]
use crate::env_variables::EnvVariables;
#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
pub fn get_search_locations(environment: &EnvVariables) -> Option<PathBuf> {
    Some(
        environment
            .home
            .clone()?
            .join("AppData")
            .join("Local")
            .join("Microsoft")
            .join("WindowsApps"),
    )
}
