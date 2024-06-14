// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::path::{Path, PathBuf};

// Similar to fs::canonicalize, but ignores UNC paths and returns the path as is (for windows).
// Usefulfor windows to ensure we have the paths in the right casing.
// For unix, this is a noop.
pub fn norm_case<P: AsRef<Path>>(path: P) -> PathBuf {
    // On unix do not use canonicalize, results in weird issues with homebrew paths
    // Even readlink does the same thing
    // Running readlink for a path thats not a symlink ends up returning relative paths for some reason.
    // A better solution is to first check if a path is a symlink and then resolve it.
    #[cfg(unix)]
    return path.as_ref().to_path_buf();

    #[cfg(windows)]
    use std::fs;

    #[cfg(windows)]
    if let Ok(resolved) = fs::canonicalize(&path) {
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

// Resolves symlinks to the real file.
// If the real file == exe, then it is not a symlink.
pub fn resolve_symlink(exe: &Path) -> Option<PathBuf> {
    let name = exe.file_name()?.to_string_lossy();
    // In bin directory of homebrew, we have files like python-build, python-config, python3-config
    if name.ends_with("-config") || name.ends_with("-build") {
        return None;
    }
    // We support resolving conda symlinks.
    if !name.starts_with("python") && !name.starts_with("conda") {
        return None;
    }

    // Running readlink for a path thats not a symlink ends up returning relative paths for some reason.
    // A better solution is to first check if a path is a symlink and then resolve it.
    let metadata = std::fs::symlink_metadata(exe).ok()?;
    if metadata.is_file() || !metadata.file_type().is_symlink() {
        return None;
    }
    if let Ok(readlink) = std::fs::canonicalize(exe) {
        if readlink == exe {
            None
        } else {
            Some(readlink)
        }
    } else {
        None
    }
}
