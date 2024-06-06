// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use regex::Regex;
use std::{
    fs,
    path::{Path, PathBuf},
};

lazy_static! {
    static ref WINDOWS_EXE: Regex =
        Regex::new(r"python(\d+\.?)*.exe").expect("error parsing Windows executable regex");
    static ref UNIX_EXE: Regex =
        Regex::new(r"python(\d+\.?)*$").expect("error parsing Unix executable regex");
}

#[cfg(windows)]
pub fn find_executable(env_path: &Path) -> Option<PathBuf> {
    [
        env_path.join("Scripts").join("python.exe"),
        env_path.join("Scripts").join("python3.exe"),
        env_path.join("python.exe"),
        env_path.join("python3.exe"),
    ]
    .into_iter()
    .find(|path| fs::metadata(path).is_ok())
}

#[cfg(unix)]
pub fn find_executable(env_path: &Path) -> Option<PathBuf> {
    [
        env_path.join("bin").join("python"),
        env_path.join("bin").join("python3"),
        env_path.join("python"),
        env_path.join("python3"),
    ]
    .into_iter()
    .find(|path| fs::metadata(path).is_ok())
}

pub fn find_executables(env_path: &Path) -> Vec<PathBuf> {
    let mut python_executables = vec![];
    let bin = if cfg!(windows) { "Scripts" } else { "bin" };
    let mut env_path = env_path.to_path_buf();
    if env_path.join(bin).metadata().is_ok() {
        env_path = env_path.join(bin);
    }

    // If we have python.exe or python3.exe, then enumerator files in this directory
    // We might have others like python 3.10 and python 3.11
    // If we do not find python or python3, then do not enumerate, as its very expensive.
    // This fn gets called from a number of places, e.g. to look scan all folders that are in PATH variable,
    // & a few others, and scanning all of those dirs is every expensive.
    let python_exe = if cfg!(windows) {
        "python.exe"
    } else {
        "python"
    };
    let python3_exe = if cfg!(windows) {
        "python3.exe"
    } else {
        "python3"
    };

    if env_path.join(python_exe).metadata().is_ok() || env_path.join(python3_exe).metadata().is_ok()
    {
        // Enumerate this directory and get all `python` & `pythonX.X` files.
        if let Ok(entries) = fs::read_dir(env_path) {
            for entry in entries.filter_map(Result::ok) {
                let file = entry.path();
                if let Ok(metadata) = fs::metadata(&file) {
                    if is_python_executable_name(&entry.path()) && metadata.is_file() {
                        python_executables.push(file);
                    }
                }
            }
        }
    }

    python_executables
}

fn is_python_executable_name(exe: &Path) -> bool {
    let name = exe
        .file_name()
        .unwrap_or_default()
        .to_str()
        .unwrap_or_default()
        .to_lowercase();
    if !name.starts_with("python") {
        return false;
    }
    // Regex to match pythonX.X.exe
    if cfg!(windows) {
        WINDOWS_EXE.is_match(&name)
    } else {
        UNIX_EXE.is_match(&name)
    }
}

// Given a list of executables, return the one with the shortest path.
// The shortest path is the most likely to be most user friendly.
pub fn get_shortest_executable(exes: &Option<Vec<PathBuf>>) -> Option<PathBuf> {
    // Ensure the executable always points to the shorted path.
    if let Some(mut exes) = exes.clone() {
        exes.sort_by(|a, b| {
            a.to_str()
                .unwrap_or_default()
                .len()
                .cmp(&b.to_str().unwrap_or_default().len())
        });
        if exes.is_empty() {
            return None;
        }
        Some(exes[0].clone())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_python_executable_test() {
        #[cfg(unix)]
        assert!(is_python_executable_name(PathBuf::from("python").as_path()));
        #[cfg(unix)]
        assert!(is_python_executable_name(
            PathBuf::from("python3").as_path()
        ));
        #[cfg(unix)]
        assert!(is_python_executable_name(
            PathBuf::from("python3.1").as_path()
        ));
        #[cfg(unix)]
        assert!(is_python_executable_name(
            PathBuf::from("python3.10").as_path()
        ));
        #[cfg(unix)]
        assert!(is_python_executable_name(
            PathBuf::from("python4.10").as_path()
        ));

        #[cfg(windows)]
        assert!(is_python_executable_name(
            PathBuf::from("python.exe").as_path()
        ));
        #[cfg(windows)]
        assert!(is_python_executable_name(
            PathBuf::from("python3.exe").as_path()
        ));
        #[cfg(windows)]
        assert!(is_python_executable_name(
            PathBuf::from("python3.1.exe").as_path()
        ));
        #[cfg(windows)]
        assert!(is_python_executable_name(
            PathBuf::from("python3.10.exe").as_path()
        ));
        #[cfg(windows)]
        assert!(is_python_executable_name(
            PathBuf::from("python4.10.exe").as_path()
        ));
    }
    #[test]
    fn is_not_python_executable_test() {
        #[cfg(unix)]
        assert!(!is_python_executable_name(
            PathBuf::from("pythonw").as_path()
        ));
        #[cfg(unix)]
        assert!(!is_python_executable_name(
            PathBuf::from("pythonw3").as_path()
        ));

        #[cfg(windows)]
        assert!(!is_python_executable_name(
            PathBuf::from("pythonw.exe").as_path()
        ));
        #[cfg(windows)]
        assert!(!is_python_executable_name(
            PathBuf::from("pythonw3.exe").as_path()
        ));
    }
}
