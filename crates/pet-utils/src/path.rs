// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn get_absolute_path(path: &Path) -> PathBuf {
    // Return the path as is.
    if cfg!(unix) {
        return path.to_path_buf();
    }
    let has_unc_prefix = path.to_string_lossy().starts_with(r"\\?\");
    if let Ok(resolved) = fs::canonicalize(path) {
        if resolved.to_string_lossy().starts_with(r"\\?\") && !has_unc_prefix {
            // If the resolved path has a UNC prefix, but the original path did not,
            // we need to remove the UNC prefix.
            PathBuf::from(resolved.to_string_lossy().trim_start_matches(r"\\?\"))
        } else {
            resolved
        }
    } else {
        path.to_path_buf()
    }
}
