// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use pet_core::os_environment::Environment;
use std::path::PathBuf;

pub fn get_search_paths_from_env_variables(environment: &dyn Environment) -> Vec<PathBuf> {
    // Exclude files from this folder, as they would have been discovered elsewhere (widows_store)
    // Also the exe is merely a pointer to another file.
    #[allow(unused_variables)]
    if let Some(home) = environment.get_user_home() {
        #[cfg(windows)]
        let apps_path = home
            .join("AppData")
            .join("Local")
            .join("Microsoft")
            .join("WindowsApps");

        let invalid_search_paths_on_unix = [
            "/var/run/com.apple.security.cryptexd/codex.system/bootstrap/usr/local/bin",
            "/var/run/com.apple.security.cryptexd/codex.system/bootstrap/usr/bin",
            "/var/run/com.apple.security.cryptexd/codex.system/bootstrap/usr/appleinternal/bin",
            "/usr/local/share/dotnet",
        ];
        let invalid_search_paths_suffixes_on_unix = [
            ".juliaup/bin",
            ".deno/bin",
            ".pyenv/shims",
            ".dotnet/tools",
            ".cargo/bin",
        ];
        #[cfg(unix)]
        let filter_path = |path: &PathBuf| {
            // This is a special folder on Mac, will not hold Python Envs.
            let p = path.to_str().unwrap_or_default();
            if invalid_search_paths_on_unix.contains(&p) {
                return false;
            }
            !invalid_search_paths_suffixes_on_unix
                .iter()
                .any(|s| p.ends_with(s))
        };

        #[cfg(windows)]
        let filter_path = |path: &PathBuf| !path.starts_with(apps_path.clone());

        environment
            .get_know_global_search_locations()
            .clone()
            .into_iter()
            .filter(filter_path)
            .collect::<Vec<PathBuf>>()
    } else {
        Vec::new()
    }
}
