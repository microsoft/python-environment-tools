// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use pet_core::os_environment::Environment;
use std::collections::HashSet;
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
            .map(normalize_search_path)
            .collect::<HashSet<PathBuf>>()
            .into_iter()
            .filter(|p| !p.starts_with(apps_path.clone()))
            .collect()
    } else {
        Vec::new()
    }
}

/// Normalizes a search path for deduplication purposes.
///
/// On Unix: Uses fs::canonicalize to resolve symlinks. This is important for merged-usr
/// systems where /bin, /sbin, /usr/sbin are symlinks to /usr/bin - we don't want to
/// report the same Python installation multiple times.
/// See: https://github.com/microsoft/python-environment-tools/pull/200
///
/// On Windows: Uses norm_case (GetLongPathNameW) to normalize case WITHOUT resolving
/// directory junctions. This is important for tools like Scoop that use junctions
/// (e.g., python\current -> python\3.13.3). Using fs::canonicalize would resolve
/// the junction, causing symlink tracking to fail when the shim points to the
/// junction path but executables are discovered from the resolved path.
/// See: https://github.com/microsoft/python-environment-tools/issues/187
fn normalize_search_path(path: PathBuf) -> PathBuf {
    #[cfg(unix)]
    {
        std::fs::canonicalize(&path).unwrap_or(path)
    }

    #[cfg(windows)]
    {
        pet_fs::path::norm_case(&path)
    }
}
