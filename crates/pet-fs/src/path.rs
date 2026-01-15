// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    env,
    path::{Path, PathBuf},
};

// Normalizes the case of a path on Windows without resolving junctions/symlinks.
// Uses GetLongPathNameW which normalizes case but preserves junction paths.
// For unix, this is a noop.
// See: https://github.com/microsoft/python-environment-tools/issues/186
pub fn norm_case<P: AsRef<Path>>(path: P) -> PathBuf {
    // On unix do not use canonicalize, results in weird issues with homebrew paths
    // Even readlink does the same thing
    // Running readlink for a path thats not a symlink ends up returning relative paths for some reason.
    // A better solution is to first check if a path is a symlink and then resolve it.
    #[cfg(unix)]
    return path.as_ref().to_path_buf();

    #[cfg(windows)]
    {
        // First, convert to absolute path if relative, without resolving symlinks/junctions
        let absolute_path = if path.as_ref().is_absolute() {
            path.as_ref().to_path_buf()
        } else if let Ok(abs) = std::env::current_dir() {
            abs.join(path.as_ref())
        } else {
            path.as_ref().to_path_buf()
        };

        // Use GetLongPathNameW to normalize case without resolving junctions
        normalize_case_windows(&absolute_path).unwrap_or_else(|| path.as_ref().to_path_buf())
    }
}

/// Windows-specific path case normalization using GetLongPathNameW.
/// This normalizes the case of path components but does NOT resolve junctions or symlinks.
#[cfg(windows)]
fn normalize_case_windows(path: &Path) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetLongPathNameW;

    // Convert path to wide string (UTF-16) with null terminator
    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // First call to get required buffer size
    let required_len = unsafe { GetLongPathNameW(wide_path.as_ptr(), std::ptr::null_mut(), 0) };

    if required_len == 0 {
        // GetLongPathNameW failed, return None
        return None;
    }

    // Allocate buffer and get the normalized path
    let mut buffer: Vec<u16> = vec![0; required_len as usize];
    let actual_len =
        unsafe { GetLongPathNameW(wide_path.as_ptr(), buffer.as_mut_ptr(), required_len) };

    if actual_len == 0 || actual_len > required_len {
        // Call failed or buffer too small
        return None;
    }

    // Truncate buffer to actual length (excluding null terminator)
    buffer.truncate(actual_len as usize);

    // Convert back to PathBuf
    let os_string = OsString::from_wide(&buffer);
    let result = PathBuf::from(os_string);

    // Remove UNC prefix if original path didn't have it
    // GetLongPathNameW may add \\?\ prefix in some cases
    let result_str = result.to_string_lossy();
    let original_has_unc = path.to_string_lossy().starts_with(r"\\?\");

    if result_str.starts_with(r"\\?\") && !original_has_unc {
        Some(PathBuf::from(result_str.trim_start_matches(r"\\?\")))
    } else {
        Some(result)
    }
}

// Resolves symlinks to the real file.
// If the real file == exe, then it is not a symlink.
pub fn resolve_symlink<T: AsRef<Path>>(exe: &T) -> Option<PathBuf> {
    let name = exe.as_ref().file_name()?.to_string_lossy();
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
        if readlink == exe.as_ref().to_path_buf() {
            None
        } else {
            Some(readlink)
        }
    } else {
        None
    }
}

pub fn expand_path(path: PathBuf) -> PathBuf {
    if path.starts_with("~") {
        if let Some(ref home) = get_user_home() {
            if let Ok(path) = path.strip_prefix("~") {
                return home.join(path);
            } else {
                return path;
            }
        }
    }

    // Specifically for https://docs.conda.io/projects/conda/en/23.1.x/user-guide/configuration/use-condarc.html#expansion-of-environment-variables
    if path.to_str().unwrap_or_default().contains("${USERNAME}")
        || path.to_str().unwrap_or_default().contains("${HOME}")
    {
        let username = env::var("USERNAME")
            .or(env::var("USER"))
            .unwrap_or_default();
        let home = env::var("HOME")
            .or_else(|_| env::var("USERPROFILE"))
            .unwrap_or_default();
        return PathBuf::from(
            path.to_str()
                .unwrap()
                .replace("${USERNAME}", &username)
                .replace("${HOME}", &home),
        );
    }
    path
}

fn get_user_home() -> Option<PathBuf> {
    let home = env::var("HOME").or_else(|_| env::var("USERPROFILE"));
    match home {
        Ok(home) => Some(norm_case(PathBuf::from(home))),
        Err(_) => None,
    }
}
