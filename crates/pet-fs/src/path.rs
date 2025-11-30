// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    env,
    path::{Path, PathBuf},
};

// Similar to fs::canonicalize, but does not resolve junctions/symlinks on Windows.
// Useful for Windows to ensure we have the paths in the right casing.
// For unix, this is a noop.
pub fn norm_case<P: AsRef<Path>>(path: P) -> PathBuf {
    // On unix do not use canonicalize, results in weird issues with homebrew paths
    // Even readlink does the same thing
    // Running readlink for a path thats not a symlink ends up returning relative paths for some reason.
    // A better solution is to first check if a path is a symlink and then resolve it.
    #[cfg(unix)]
    return path.as_ref().to_path_buf();

    #[cfg(windows)]
    {
        // Use GetLongPathNameW to normalize case without resolving junctions/symlinks
        // This preserves user-provided paths when they go through junctions
        // (e.g., Windows Store Python, user junctions from C: to S: drive)
        get_long_path_name(path.as_ref()).unwrap_or_else(|| path.as_ref().to_path_buf())
    }
}

/// Uses Windows GetLongPathNameW API to normalize path casing
/// without resolving symlinks or junctions.
#[cfg(windows)]
fn get_long_path_name(path: &Path) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetLongPathNameW;

    // Convert path to wide string (null-terminated)
    let wide_path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // First call to get required buffer size
    let required_len = unsafe { GetLongPathNameW(wide_path.as_ptr(), std::ptr::null_mut(), 0) };
    if required_len == 0 {
        return None;
    }

    // Allocate buffer and get the long path name
    let mut buffer: Vec<u16> = vec![0; required_len as usize];
    let result = unsafe { GetLongPathNameW(wide_path.as_ptr(), buffer.as_mut_ptr(), required_len) };

    if result == 0 || result > required_len {
        return None;
    }

    // Trim the null terminator and convert back to PathBuf
    buffer.truncate(result as usize);
    Some(PathBuf::from(OsString::from_wide(&buffer)))
}

/// Checks if the given path is a Windows junction (mount point).
/// Junctions are directory reparse points with IO_REPARSE_TAG_MOUNT_POINT.
/// Returns false on non-Windows platforms or if the path is a regular symlink.
#[cfg(windows)]
pub fn is_junction<P: AsRef<Path>>(path: P) -> bool {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        FileAttributeTagInfo, GetFileInformationByHandleEx, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA0000003;

    #[repr(C)]
    struct FILE_ATTRIBUTE_TAG_INFO {
        file_attributes: u32,
        reparse_tag: u32,
    }

    // Check if it's a reparse point first using metadata
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(_) => return false,
    };

    // Use file_attributes to check for reparse point
    use std::os::windows::fs::MetadataExt;
    let attrs = metadata.file_attributes();
    if attrs & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return false;
    }

    // Open the file/directory to get the reparse tag
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&path)
    {
        Ok(f) => f,
        Err(_) => return false,
    };

    let handle = file.as_raw_handle();
    if handle as isize == INVALID_HANDLE_VALUE as isize {
        return false;
    }

    let mut tag_info = FILE_ATTRIBUTE_TAG_INFO {
        file_attributes: 0,
        reparse_tag: 0,
    };

    let success = unsafe {
        GetFileInformationByHandleEx(
            handle as *mut _,
            FileAttributeTagInfo,
            &mut tag_info as *mut _ as *mut _,
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };

    if success == 0 {
        return false;
    }

    // IO_REPARSE_TAG_MOUNT_POINT indicates a junction
    tag_info.reparse_tag == IO_REPARSE_TAG_MOUNT_POINT
}

#[cfg(not(windows))]
pub fn is_junction<P: AsRef<Path>>(_path: P) -> bool {
    // Junctions only exist on Windows
    false
}

/// Checks if any component of the given path traverses through a junction.
/// This is useful for determining if a path was accessed via a junction.
#[cfg(windows)]
pub fn path_contains_junction<P: AsRef<Path>>(path: P) -> bool {
    let path = path.as_ref();
    let mut current = PathBuf::new();

    for component in path.components() {
        current.push(component);
        if current.exists() && is_junction(&current) {
            return true;
        }
    }
    false
}

#[cfg(not(windows))]
pub fn path_contains_junction<P: AsRef<Path>>(_path: P) -> bool {
    false
}

// Resolves symlinks to the real file.
// If the real file == exe, then it is not a symlink.
// Note: Windows junctions are NOT resolved - only true symlinks are resolved.
// This preserves user-provided paths that traverse through junctions.
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

    // On Windows, check if this is a junction - we don't want to resolve junctions
    // as they may point to system-only locations (e.g., Windows Store Python)
    // or the user may have set up junctions intentionally to map drives.
    #[cfg(windows)]
    if is_junction(exe) {
        return None;
    }

    // Also check if any parent directory is a junction - if so, don't resolve
    // as the user's path should be preserved.
    #[cfg(windows)]
    if path_contains_junction(exe) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_norm_case_returns_path_unchanged_on_nonexistent() {
        // For non-existent paths, norm_case should return the original path
        let path = PathBuf::from("/nonexistent/path/to/python");
        let result = norm_case(&path);
        assert_eq!(result, path);
    }

    #[test]
    fn test_is_junction_returns_false_for_regular_file() {
        // Create a temp file and verify it's not detected as a junction
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_junction_check.txt");
        std::fs::write(&test_file, "test").ok();

        assert!(!is_junction(&test_file));

        // Cleanup
        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_is_junction_returns_false_for_regular_directory() {
        // Regular directories should not be detected as junctions
        let temp_dir = std::env::temp_dir();
        assert!(!is_junction(&temp_dir));
    }

    #[test]
    fn test_is_junction_returns_false_for_nonexistent_path() {
        let path = PathBuf::from("/nonexistent/path");
        assert!(!is_junction(&path));
    }

    #[test]
    fn test_path_contains_junction_returns_false_for_regular_path() {
        // Regular paths should not be detected as containing junctions
        let temp_dir = std::env::temp_dir();
        assert!(!path_contains_junction(&temp_dir));
    }

    #[test]
    fn test_path_contains_junction_returns_false_for_nonexistent_path() {
        let path = PathBuf::from("/nonexistent/path/to/file");
        assert!(!path_contains_junction(&path));
    }

    #[test]
    fn test_resolve_symlink_returns_none_for_regular_file() {
        // Create a temp file named python_test to pass the name filter
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("python_test");
        std::fs::write(&test_file, "test").ok();

        // Regular files should not be resolved as symlinks
        assert!(resolve_symlink(&test_file).is_none());

        // Cleanup
        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_resolve_symlink_skips_config_files() {
        let path = PathBuf::from("/usr/bin/python-config");
        assert!(resolve_symlink(&path).is_none());

        let path2 = PathBuf::from("/usr/bin/python-build");
        assert!(resolve_symlink(&path2).is_none());
    }

    #[test]
    fn test_resolve_symlink_skips_non_python_files() {
        let path = PathBuf::from("/usr/bin/ruby");
        assert!(resolve_symlink(&path).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_norm_case_is_noop_on_unix() {
        // On Unix, norm_case should return the path unchanged
        let path = PathBuf::from("/usr/bin/python3");
        let result = norm_case(&path);
        assert_eq!(result, path);
    }

    #[cfg(unix)]
    #[test]
    fn test_is_junction_always_false_on_unix() {
        // Junctions don't exist on Unix
        let path = PathBuf::from("/usr/bin");
        assert!(!is_junction(&path));
    }
}
