// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use pet_utils::executable::resolve_symlink;
use regex::Regex;
use std::path::{Path, PathBuf};

lazy_static! {
    static ref PYTHON_VERSION: Regex =
        Regex::new(r"/python@((\d+\.?)*)/").expect("error parsing Version regex for Homebrew");
}

pub fn get_known_symlinks(python_exe: &Path, full_version: &String) -> Vec<PathBuf> {
    if python_exe.starts_with("/opt/homebrew/Cellar") {
        // Real exe - /opt/homebrew/Cellar/python@3.12/3.12.3/Frameworks/Python.framework/Versions/3.12/bin/python3.12

        // Known symlinks include
        // /opt/homebrew/bin/python3.12
        // /opt/homebrew/opt/python3/bin/python3.12
        // /opt/homebrew/Cellar/python@3.12/3.12.3/bin/python3.12
        // /opt/homebrew/opt/python@3.12/bin/python3.12
        // /opt/homebrew/Cellar/python@3.12/3.12.3/Frameworks/Python.framework/Versions/3.12/bin/python3.12
        // /opt/homebrew/Cellar/python@3.12/3.12.3/Frameworks/Python.framework/Versions/Current/bin/python3.12
        // /opt/homebrew/Frameworks/Python.framework/Versions/3.12/bin/python3.12
        // /opt/homebrew/Frameworks/Python.framework/Versions/Current/bin/python3.12
        // /opt/homebrew/Cellar/python@3.12/3.12.3/Frameworks/Python.framework/Versions/3.12/bin/python3.12
        match PYTHON_VERSION.captures(python_exe.to_str().unwrap_or_default()) {
            Some(captures) => match captures.get(1) {
                Some(version) => {
                    let version = version.as_str().to_string();
                    let mut symlinks = vec![
                        PathBuf::from(format!("/opt/homebrew/bin/python{}", version)),
                        PathBuf::from(format!("/opt/homebrew/opt/python3/bin/python{}",version)),
                        PathBuf::from(format!("/opt/homebrew/Cellar/python@{}/{}/bin/python{}",version,  full_version, version)),
                        PathBuf::from(format!("/opt/homebrew/opt/python@{}/bin/python{}", version, version)),
                        PathBuf::from(format!("/opt/homebrew/Cellar/python@{}/{}/Frameworks/Python.framework/Versions/{}/bin/python{}", version, full_version, version, version)),
                        PathBuf::from(format!("/opt/homebrew/Cellar/python@{}/{}/Frameworks/Python.framework/Versions/Current/bin/python{}", version, full_version, version)),
                        PathBuf::from(format!("/opt/homebrew/Frameworks/Python.framework/Versions/{}/bin/python{}", version, version)),
                        PathBuf::from(format!("/opt/homebrew/Frameworks/Python.framework/Versions/Current/bin/python{}", version)),
                        PathBuf::from(format!("/opt/homebrew/Cellar/python@{}/{}/Frameworks/Python.framework/Versions/{}/bin/python{}",version, full_version, version, version)),
                        ];

                    // Check if this symlink is pointing to the same place as the resolved python exe
                    let another_symlink = PathBuf::from("/opt/homebrew/bin/python3");
                    if resolve_symlink(&another_symlink).is_some() {
                        symlinks.push(another_symlink);
                    }
                    // Check if this symlink is pointing to the same place as the resolved python exe
                    let another_symlink = PathBuf::from("/opt/homebrew/bin/python");
                    if resolve_symlink(&another_symlink).is_some() {
                        symlinks.push(another_symlink);
                    }
                    symlinks
                }
                None => vec![],
            },
            None => vec![],
        }
    } else if python_exe.starts_with("/usr/local/Cellar") {
        // Real exe - /usr/local/Cellar/python@3.8/3.8.19/Frameworks/Python.framework/Versions/3.8/bin/python3.8

        // Known symlinks include
        // /usr/local/bin/python3.8
        // /usr/local/opt/python@3.8/bin/python3.8
        // /usr/local/Cellar/python@3.8/3.8.19/bin/python3.8
        // /usr/local/Cellar/python@3.8/3.8.19/Frameworks/Python.framework/Versions/3.8/bin/python3.8
        match PYTHON_VERSION.captures(python_exe.to_str().unwrap_or_default()) {
            Some(captures) => match captures.get(1) {
                Some(version) => {
                    let version = version.as_str().to_string();
                    // Never include `/usr/local/bin/python` into this list.
                    // See previous explanation

                    let mut symlinks = vec![
                        PathBuf::from(format!(
                            "/usr/local/opt/python@{}/bin/python{}",
                            version, version
                        )),
                        PathBuf::from(format!(
                            "/usr/local/Cellar/python@{}/{}/bin/python{}",
                            version, full_version, version
                        )),
                        PathBuf::from(format!(
                            "/usr/local/Cellar/python@{}/{}/Frameworks/Python.framework/Versions/{}/bin/python{}",
                            version, full_version, version, version
                        )),
                    ];

                    let user_bin_symlink =
                        PathBuf::from(format!("/usr/local/bin/python{}", version));
                    // This is a special folder, if users install python using other means, this file
                    // might get overridden. So we should only add this if this files points to the same place
                    if resolve_symlink(&user_bin_symlink).is_some() {
                        symlinks.push(user_bin_symlink);
                    }
                    // Check if this symlink is pointing to the same place as the resolved python exe
                    let another_symlink = PathBuf::from("/usr/local/bin/python3");
                    if resolve_symlink(&another_symlink).is_some() {
                        symlinks.push(another_symlink);
                    }
                    // Check if this symlink is pointing to the same place as the resolved python exe
                    let another_symlink = PathBuf::from("/usr/local/bin/python");
                    if resolve_symlink(&another_symlink).is_some() {
                        symlinks.push(another_symlink);
                    }

                    symlinks
                }
                None => vec![],
            },
            None => vec![],
        }
    } else if python_exe.starts_with("/home/linuxbrew/.linuxbrew/Cellar") {
        // Real exe - /home/linuxbrew/.linuxbrew/Cellar/python@3.12/3.12.3/bin/python3.12

        // Known symlinks include
        // /usr/local/bin/python3.12
        // /home/linuxbrew/.linuxbrew/bin/python3.12
        // /home/linuxbrew/.linuxbrew/opt/python@3.12/bin/python3.12
        match PYTHON_VERSION.captures(python_exe.to_str().unwrap_or_default()) {
            Some(captures) => match captures.get(1) {
                Some(version) => {
                    let version = version.as_str().to_string();
                    // Never include `/usr/local/bin/python` into this list.
                    // See previous explanation
                    let mut symlinks = vec![
                        PathBuf::from(format!("/home/linuxbrew/.linuxbrew/bin/python{}", version)),
                        PathBuf::from(format!(
                            "/home/linuxbrew/.linuxbrew/opt/python@{}/bin/python{}",
                            version, version
                        )),
                    ];

                    let user_bin_symlink =
                        PathBuf::from(format!("/usr/local/bin/python{}", version));
                    // This is a special folder, if users install python using other means, this file
                    // might get overridden. So we should only add this if this files points to the same place
                    if resolve_symlink(&user_bin_symlink).is_some() {
                        symlinks.push(user_bin_symlink);
                    }
                    // Check if this symlink is pointing to the same place as the resolved python exe
                    let another_symlink = PathBuf::from("/usr/local/bin/python3");
                    if resolve_symlink(&another_symlink).is_some() {
                        symlinks.push(another_symlink);
                    }
                    // Check if this symlink is pointing to the same place as the resolved python exe
                    let another_symlink = PathBuf::from("/usr/local/bin/python");
                    if resolve_symlink(&another_symlink).is_some() {
                        symlinks.push(another_symlink);
                    }

                    symlinks
                }
                None => vec![],
            },
            None => vec![],
        }
    } else {
        vec![]
    }
}
