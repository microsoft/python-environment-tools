// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! WinPython environment locator for Windows.
//!
//! WinPython is a portable Python distribution for Windows that is commonly used
//! in scientific and educational environments. This locator detects WinPython
//! installations by looking for characteristic directory structures and marker files.

use lazy_static::lazy_static;
use log::trace;
use pet_core::{
    env::PythonEnv,
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind},
    reporter::Reporter,
    Locator, LocatorKind,
};
use pet_fs::path::norm_case;
use pet_python_utils::executable::find_executables;
use pet_virtualenv::is_virtualenv;
use regex::Regex;
use std::{
    fs,
    path::{Path, PathBuf},
};

lazy_static! {
    /// Regex to match WinPython top-level directory names.
    /// Examples: WPy64-31300, WPy32-3900, WPy-31100, WPy64-31300Qt5
    static ref WINPYTHON_DIR_REGEX: Regex =
        Regex::new(r"(?i)^WPy(64|32)?-?\d+").expect("error parsing WinPython directory regex");

    /// Regex to match Python folder within WinPython.
    /// Examples: python-3.13.0.amd64, python-3.9.0, python-3.10.5.amd64
    static ref PYTHON_FOLDER_REGEX: Regex =
        Regex::new(r"(?i)^python-\d+\.\d+\.\d+(\.(amd64|win32))?$")
            .expect("error parsing Python folder regex");
}

/// Marker files that indicate a WinPython installation.
const WINPYTHON_MARKER_FILES: &[&str] = &[".winpython", "winpython.ini"];

pub struct WinPython {}

impl WinPython {
    pub fn new() -> WinPython {
        WinPython {}
    }
}

impl Default for WinPython {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a directory is a WinPython installation root by looking for marker files.
fn is_winpython_root(path: &Path) -> bool {
    for marker in WINPYTHON_MARKER_FILES {
        if path.join(marker).exists() {
            return true;
        }
    }
    false
}

/// Check if a directory name matches the WinPython naming pattern.
fn is_winpython_dir_name(name: &str) -> bool {
    WINPYTHON_DIR_REGEX.is_match(name)
}

/// Check if a directory name matches the Python folder naming pattern within WinPython.
fn is_python_folder_name(name: &str) -> bool {
    PYTHON_FOLDER_REGEX.is_match(name)
}

/// Given a Python executable path, try to find the WinPython root directory.
/// Returns (winpython_root, python_folder) if found.
fn find_winpython_root(executable: &Path) -> Option<(PathBuf, PathBuf)> {
    // Typical structure:
    // WPy64-31300/python-3.13.0.amd64/python.exe
    // or
    // WPy64-31300/python-3.13.0.amd64/Scripts/python.exe (unlikely but possible)

    let mut current = executable.parent()?;

    // Walk up the directory tree looking for WinPython markers
    for _ in 0..5 {
        // Check if current directory has WinPython marker files
        if is_winpython_root(current) {
            // Find the python folder within this WinPython root
            if let Some(python_folder) = find_python_folder_in_winpython(current) {
                return Some((current.to_path_buf(), python_folder));
            }
        }

        // Check if parent directory name matches WinPython pattern
        if let Some(name) = current.file_name() {
            let name_str = name.to_string_lossy();
            if is_winpython_dir_name(&name_str) {
                // This might be the WinPython root
                if let Some(python_folder) = find_python_folder_in_winpython(current) {
                    return Some((current.to_path_buf(), python_folder));
                }
            }
        }

        // Move to parent directory
        current = current.parent()?;
    }

    None
}

/// Find the Python installation folder within a WinPython root directory.
fn find_python_folder_in_winpython(winpython_root: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(winpython_root).ok()?;

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name() {
                let name_str = name.to_string_lossy();
                if is_python_folder_name(&name_str) {
                    // Verify this folder contains python.exe
                    let python_exe = path.join(if cfg!(windows) {
                        "python.exe"
                    } else {
                        "python"
                    });
                    if python_exe.exists() {
                        return Some(path);
                    }
                }
            }
        }
    }

    None
}

/// Get the version from the Python folder name.
/// Example: "python-3.13.0.amd64" -> "3.13.0"
fn version_from_folder_name(folder_name: &str) -> Option<String> {
    let name = folder_name.to_lowercase();
    if let Some(stripped) = name.strip_prefix("python-") {
        // Remove architecture suffix if present
        let version_part = stripped
            .strip_suffix(".amd64")
            .or_else(|| stripped.strip_suffix(".win32"))
            .unwrap_or(stripped);
        Some(version_part.to_string())
    } else {
        None
    }
}

/// Get the display name for a WinPython installation.
fn get_display_name(winpython_root: &Path, version: Option<&str>) -> Option<String> {
    let folder_name = winpython_root.file_name()?.to_string_lossy().to_string();

    if let Some(ver) = version {
        Some(format!("WinPython {ver}"))
    } else {
        Some(format!("WinPython ({folder_name})"))
    }
}

impl Locator for WinPython {
    fn get_kind(&self) -> LocatorKind {
        LocatorKind::WinPython
    }

    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::WinPython]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        // WinPython is Windows-only
        if cfg!(not(windows)) {
            return None;
        }

        // Don't identify virtual environments as WinPython
        if is_virtualenv(env) {
            return None;
        }

        // Try to find the WinPython root from the executable path
        let (winpython_root, python_folder) = find_winpython_root(&env.executable)?;

        trace!(
            "Found WinPython installation at {:?} (python folder: {:?})",
            winpython_root,
            python_folder
        );

        // Get version from folder name or pyvenv.cfg
        let version = python_folder
            .file_name()
            .and_then(|n| version_from_folder_name(&n.to_string_lossy()))
            .or_else(|| env.version.clone());

        // Collect all Python executables in the installation
        let mut symlinks = vec![env.executable.clone()];

        // Add executables from the python folder root
        for exe in find_executables(&python_folder) {
            if !symlinks.contains(&exe) {
                symlinks.push(norm_case(&exe));
            }
        }

        // Add executables from Scripts directory
        let scripts_dir = python_folder.join("Scripts");
        if scripts_dir.exists() {
            for exe in find_executables(&scripts_dir) {
                let exe_name = exe.file_name().map(|n| n.to_string_lossy().to_lowercase());
                // Only include python executables, not other scripts
                if exe_name
                    .as_ref()
                    .is_some_and(|n| n.starts_with("python") && !n.contains("pip"))
                    && !symlinks.contains(&exe)
                {
                    symlinks.push(norm_case(&exe));
                }
            }
        }

        symlinks.sort();
        symlinks.dedup();

        let display_name = get_display_name(&winpython_root, version.as_deref());

        Some(
            PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::WinPython))
                .display_name(display_name)
                .executable(Some(env.executable.clone()))
                .version(version)
                .prefix(Some(python_folder))
                .symlinks(Some(symlinks))
                .build(),
        )
    }

    fn find(&self, reporter: &dyn Reporter) {
        // WinPython is Windows-only
        if cfg!(not(windows)) {
            return;
        }

        // WinPython installations are typically found in user-chosen locations.
        // Unlike other Python distributions, there's no standard installation path.
        // Common locations include:
        // - User's home directory
        // - Desktop
        // - Downloads folder
        // - Custom directories
        //
        // We search in common locations where users might extract WinPython.
        let search_paths = get_winpython_search_paths();

        for search_path in search_paths {
            if !search_path.exists() {
                continue;
            }

            trace!("Searching for WinPython in {:?}", search_path);

            // Look for WinPython directories
            if let Ok(entries) = fs::read_dir(&search_path) {
                for entry in entries.filter_map(Result::ok) {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }

                    // Check if this directory is a WinPython installation
                    if let Some(name) = path.file_name() {
                        let name_str = name.to_string_lossy();
                        if is_winpython_dir_name(&name_str) || is_winpython_root(&path) {
                            if let Some(python_folder) = find_python_folder_in_winpython(&path) {
                                let python_exe = python_folder.join(if cfg!(windows) {
                                    "python.exe"
                                } else {
                                    "python"
                                });

                                if python_exe.exists() {
                                    let env = PythonEnv::new(python_exe, Some(python_folder), None);
                                    if let Some(found_env) = self.try_from(&env) {
                                        reporter.report_environment(&found_env);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Get common paths where WinPython installations might be located.
#[cfg(windows)]
fn get_winpython_search_paths() -> Vec<PathBuf> {
    use std::env;

    let mut paths = Vec::new();

    // User's home directory
    if let Ok(home) = env::var("USERPROFILE") {
        let home_path = PathBuf::from(&home);
        paths.push(home_path.clone());
        paths.push(home_path.join("Desktop"));
        paths.push(home_path.join("Downloads"));
        paths.push(home_path.join("Documents"));
        paths.push(home_path.join("WinPython"));
    }

    // Root of common drives
    for drive in ['C', 'D', 'E'] {
        let drive_path = PathBuf::from(format!("{}:\\", drive));
        paths.push(drive_path.clone());
        paths.push(drive_path.join("WinPython"));
        paths.push(drive_path.join("Python"));
    }

    // Program Files directories
    if let Ok(program_files) = env::var("ProgramFiles") {
        paths.push(PathBuf::from(&program_files));
    }
    if let Ok(program_files_x86) = env::var("ProgramFiles(x86)") {
        paths.push(PathBuf::from(&program_files_x86));
    }

    paths
}

#[cfg(not(windows))]
fn get_winpython_search_paths() -> Vec<PathBuf> {
    // WinPython is Windows-only, return empty on other platforms
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_is_winpython_dir_name() {
        assert!(is_winpython_dir_name("WPy64-31300"));
        assert!(is_winpython_dir_name("WPy32-3900"));
        assert!(is_winpython_dir_name("WPy-31100"));
        assert!(is_winpython_dir_name("WPy64-31300Qt5"));
        assert!(is_winpython_dir_name("wpy64-31300")); // case insensitive

        assert!(!is_winpython_dir_name("Python"));
        assert!(!is_winpython_dir_name("python-3.13.0"));
        assert!(!is_winpython_dir_name("random-folder"));
    }

    #[test]
    fn test_is_python_folder_name() {
        assert!(is_python_folder_name("python-3.13.0.amd64"));
        assert!(is_python_folder_name("python-3.9.0"));
        assert!(is_python_folder_name("python-3.10.5.amd64"));
        assert!(is_python_folder_name("python-3.8.0.win32"));
        assert!(is_python_folder_name("Python-3.13.0.amd64")); // case insensitive

        assert!(!is_python_folder_name("python"));
        assert!(!is_python_folder_name("python3"));
        assert!(!is_python_folder_name("WPy64-31300"));
    }

    #[test]
    fn test_version_from_folder_name() {
        assert_eq!(
            version_from_folder_name("python-3.13.0.amd64"),
            Some("3.13.0".to_string())
        );
        assert_eq!(
            version_from_folder_name("python-3.9.0"),
            Some("3.9.0".to_string())
        );
        assert_eq!(
            version_from_folder_name("python-3.8.0.win32"),
            Some("3.8.0".to_string())
        );
        assert_eq!(
            version_from_folder_name("Python-3.10.5.amd64"),
            Some("3.10.5".to_string())
        );

        assert_eq!(version_from_folder_name("python"), None);
        assert_eq!(version_from_folder_name("not-python-3.9.0"), None);
    }

    #[test]
    fn test_get_display_name() {
        // Use a simple directory name that works on all platforms
        let path = PathBuf::from("WPy64-31300");
        assert_eq!(
            get_display_name(&path, Some("3.13.0")),
            Some("WinPython 3.13.0".to_string())
        );
        assert_eq!(
            get_display_name(&path, None),
            Some("WinPython (WPy64-31300)".to_string())
        );
    }

    #[test]
    fn test_is_winpython_root_with_marker() {
        let dir = tempdir().unwrap();
        let winpython_marker = dir.path().join(".winpython");
        File::create(&winpython_marker).unwrap();

        assert!(is_winpython_root(dir.path()));
    }

    #[test]
    fn test_is_winpython_root_with_ini_marker() {
        let dir = tempdir().unwrap();
        let winpython_ini = dir.path().join("winpython.ini");
        File::create(&winpython_ini).unwrap();

        assert!(is_winpython_root(dir.path()));
    }

    #[test]
    fn test_is_winpython_root_without_marker() {
        let dir = tempdir().unwrap();
        assert!(!is_winpython_root(dir.path()));
    }

    #[test]
    #[cfg(windows)]
    fn test_find_python_folder_in_winpython() {
        let dir = tempdir().unwrap();
        let python_folder = dir.path().join("python-3.13.0.amd64");
        fs::create_dir_all(&python_folder).unwrap();

        // Create python.exe
        let python_exe = python_folder.join("python.exe");
        File::create(&python_exe).unwrap();

        let result = find_python_folder_in_winpython(dir.path());
        assert!(result.is_some());
        assert_eq!(result.unwrap(), python_folder);
    }

    #[test]
    fn test_find_python_folder_missing_exe() {
        let dir = tempdir().unwrap();
        let python_folder = dir.path().join("python-3.13.0.amd64");
        fs::create_dir_all(&python_folder).unwrap();

        // No python.exe created
        let result = find_python_folder_in_winpython(dir.path());
        assert!(result.is_none());
    }

    #[test]
    #[cfg(windows)]
    fn test_find_winpython_root_with_marker() {
        let dir = tempdir().unwrap();

        // Create WinPython structure with marker
        let winpython_root = dir.path().join("WPy64-31300");
        fs::create_dir_all(&winpython_root).unwrap();
        File::create(winpython_root.join(".winpython")).unwrap();

        let python_folder = winpython_root.join("python-3.13.0.amd64");
        fs::create_dir_all(&python_folder).unwrap();
        let python_exe = python_folder.join("python.exe");
        File::create(&python_exe).unwrap();

        let result = find_winpython_root(&python_exe);
        assert!(result.is_some());
        let (root, folder) = result.unwrap();
        assert_eq!(root, winpython_root);
        assert_eq!(folder, python_folder);
    }

    #[test]
    #[cfg(windows)]
    fn test_find_winpython_root_by_dir_name() {
        let dir = tempdir().unwrap();

        // Create WinPython structure without marker (relying on dir name)
        let winpython_root = dir.path().join("WPy64-31300");
        fs::create_dir_all(&winpython_root).unwrap();

        let python_folder = winpython_root.join("python-3.13.0.amd64");
        fs::create_dir_all(&python_folder).unwrap();
        let python_exe = python_folder.join("python.exe");
        File::create(&python_exe).unwrap();

        let result = find_winpython_root(&python_exe);
        assert!(result.is_some());
        let (root, folder) = result.unwrap();
        assert_eq!(root, winpython_root);
        assert_eq!(folder, python_folder);
    }

    #[test]
    fn test_find_winpython_root_not_winpython() {
        let dir = tempdir().unwrap();

        // Create a regular Python structure (not WinPython)
        let python_folder = dir.path().join("some-random-folder");
        fs::create_dir_all(&python_folder).unwrap();

        #[cfg(windows)]
        let python_exe = python_folder.join("python.exe");
        #[cfg(not(windows))]
        let python_exe = python_folder.join("python");

        File::create(&python_exe).unwrap();

        let result = find_winpython_root(&python_exe);
        assert!(result.is_none());
    }

    #[test]
    fn test_winpython_locator_kind() {
        let locator = WinPython::new();
        assert_eq!(locator.get_kind(), LocatorKind::WinPython);
    }

    #[test]
    fn test_winpython_supported_categories() {
        let locator = WinPython::new();
        let categories = locator.supported_categories();
        assert_eq!(categories.len(), 1);
        assert_eq!(categories[0], PythonEnvironmentKind::WinPython);
    }
}
