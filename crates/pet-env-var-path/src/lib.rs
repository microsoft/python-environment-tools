// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use pet_core::os_environment::Environment;
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

        // Ignore
        let mut paths_to_ignore = vec![];

        if std::env::consts::OS != "macos" && std::env::consts::OS != "windows" {
            // Ignore these as they will be found in linux global.
            paths_to_ignore.push(PathBuf::from("/bin"));
            paths_to_ignore.push(PathBuf::from("/usr/bin"));
            paths_to_ignore.push(PathBuf::from("/usr/local/bin"));
        }
        environment
            .get_know_global_search_locations()
            .clone()
            .into_iter()
            .filter(|p| !p.starts_with(apps_path.clone()))
            .filter(|p| !paths_to_ignore.contains(p))
            .collect::<Vec<PathBuf>>()
    } else {
        Vec::new()
    }
}
