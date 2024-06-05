// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    fs,
    path::{Path, PathBuf},
};

// This function is used to fix the casing of the file path.
// by returning the actual path with the correct casing as found on the OS.
// This is a noop for Unix systems.
// I.e. this function is only useful on Windows.
pub fn fix_file_path_casing(path: &Path) -> PathBuf {
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
