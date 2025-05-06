// Copyright (c) Microsoft Corporation. All rights reserved.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use log::trace;
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
        env_path.join("bin").join("python.exe"),
        env_path.join("bin").join("python3.exe"),
        env_path.join("python.exe"),
        env_path.join("python3.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
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
    .find(|path| path.is_file())
}

pub fn find_executables<T: AsRef<Path>>(env_path: T) -> Vec<PathBuf> {
    let mut env_path = env_path.as_ref().to_path_buf();
    // Never find exes in `.pyenv/shims/` folder, they are not valid exes
    if env_path.ends_with(".pyenv/shims") {
        return vec![];
    }
    let mut python_executables = vec![];
    if cfg!(windows) {
        // Only windows can have a Scripts folder
        let bin = "Scripts";
        if env_path.join(bin).exists() {
            env_path = env_path.join(bin);
        }
    }
    let bin = "bin"; // Windows can have bin as well, https://github.com/microsoft/vscode-python/issues/24792
    if env_path.join(bin).exists() {
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

    // On linux /home/linuxbrew/.linuxbrew/bin does not contain a `python` file
    // If you install python@3.10, then only a python3.10 exe is created in that bin directory.
    // As a compromise, we only enumerate if this is a bin directory and there are no python exes
    // Else enumerating entire directories is very expensive.
    if env_path.join(python_exe).exists()
        || env_path.join(python3_exe).exists()
        || env_path.ends_with(bin)
    {
        // Enumerate this directory and get all `python` & `pythonX.X` files.
        if let Ok(entries) = fs::read_dir(env_path) {
            for entry in entries.filter_map(Result::ok) {
                let file = entry.path();
                if file.is_file() && is_python_executable_name(&file) {
                    python_executables.push(file);
                }
            }
        }
    }

    // Ensure the exe `python` is first, instead of `python3.10`
    python_executables.sort();
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

pub fn should_search_for_environments_in_path<P: AsRef<Path>>(path: &P) -> bool {
    // Never search in the .git folder
    // Never search in the node_modules folder
    // Mostly copied from https://github.com/github/gitignore/blob/main/Python.gitignore
    let folders_to_ignore = [
        "node_modules",
        ".cargo",
        ".devcontainer",
        ".github",
        ".git",
        ".tox",
        ".nox",
        ".hypothesis",
        ".ipynb_checkpoints",
        ".eggs",
        ".coverage",
        ".cache",
        ".pyre",
        ".ptype",
        ".pytest_cache",
        ".vscode",
        "__pycache__",
        "__pypackages__",
        ".mypy_cache",
        "cython_debug",
        "env.bak",
        "venv.bak",
        "Scripts", // If the folder ends bin/scripts, then ignore it, as the parent is most likely an env.
        "bin", // If the folder ends bin/scripts, then ignore it, as the parent is most likely an env.
    ];
    for folder in folders_to_ignore.iter() {
        if path.as_ref().ends_with(folder) {
            trace!("Ignoring folder: {:?}", path.as_ref());
            return false;
        }
    }

    true
}
