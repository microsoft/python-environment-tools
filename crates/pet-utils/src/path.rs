// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    fs,
    path::{Path, PathBuf},
};

// Similar to fs::canonicalize, but ignores UNC paths and returns the path as is (for windows).
pub fn normalize<P: AsRef<Path>>(path: P) -> PathBuf {
    if let Ok(resolved) = fs::canonicalize(&path) {
        // Return the path as is.
        if cfg!(unix) {
            return resolved;
        }
        // Windows specific handling, https://github.com/rust-lang/rust/issues/42869
        let has_unc_prefix = path.as_ref().to_string_lossy().starts_with(r"\\?\");
        if resolved.to_string_lossy().starts_with(r"\\?\") && !has_unc_prefix {
            // If the resolved path has a UNC prefix, but the original path did not,
            // we need to remove the UNC prefix.
            PathBuf::from(resolved.to_string_lossy().trim_start_matches(r"\\?\"))
        } else {
            resolved
        }
    } else {
        path.as_ref().to_path_buf()
    }
}
