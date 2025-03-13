// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use pet_core::os_environment::Environment;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

pub fn get_search_paths_from_env_variables(environment: &dyn Environment) -> Vec<PathBuf> {
    // Exclude files from this folder, as they would have been discovered elsewhere (widows_store)
    // Also the exe is merely a pointer to another file.
    if let Some(home) = environment.get_user_home() {
        let apps_path = home
            .join("AppData")
            .join("Local")
            .join("Microsoft")
            .join("WindowsApps");

        environment
            .get_know_global_search_locations()
            .into_iter()
            .map(|p| fs::canonicalize(&p).unwrap_or(p))
            .collect::<HashSet<PathBuf>>()
            .into_iter()
            .filter(|p| !p.starts_with(apps_path.clone()))
            .collect()
    } else {
        Vec::new()
    }
}
