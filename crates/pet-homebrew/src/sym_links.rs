// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use pet_fs::path::resolve_symlink;
use pet_python_utils::executable::find_executables;
use regex::Regex;
use std::{
    fs,
    path::{Path, PathBuf},
};

lazy_static! {
    static ref PYTHON_VERSION: Regex =
        Regex::new(r"/python@((\d+\.?)*)/").expect("error parsing Version regex for Homebrew");
}

pub fn get_known_symlinks(
    symlink_resolved_python_exe: &Path,
    full_version: &String,
) -> Vec<PathBuf> {
    let mut symlinks = get_known_symlinks_impl(symlink_resolved_python_exe, full_version);

    // Go through all the exes in all of the above bin directories and verify we have a list of all of them.
    // They too could be symlinks, e.g. we could have `/opt/homebrew/bin/python3` & also `/opt/homebrew/bin/python`
    // And possible they are all symlnks to the same exe.
    let threads = symlinks
        .iter()
        .map(|symlink| {
            let symlink = symlink.clone();
            let known_symlinks = symlinks.clone();
            std::thread::spawn(move || {
                if let Some(bin) = symlink.parent() {
                    let mut symlinks = vec![];
                    for possible_symlink in find_executables(bin) {
                        if let Some(symlink) = resolve_symlink(&possible_symlink) {
                            if known_symlinks.contains(&symlink) {
                                symlinks.push(possible_symlink);
                            }
                        }
                    }
                    symlinks
                } else {
                    vec![]
                }
            })
        })
        .collect::<Vec<_>>();
    let other_symlinks = threads
        .into_iter()
        .flat_map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    symlinks.extend(other_symlinks);

    symlinks.sort();
    symlinks.dedup();

    symlinks
}

pub fn get_known_symlinks_impl(
    symlink_resolved_python_exe: &Path,
    full_version: &String,
) -> Vec<PathBuf> {
    if symlink_resolved_python_exe.starts_with("/opt/homebrew/Cellar") {
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
        match PYTHON_VERSION.captures(symlink_resolved_python_exe.to_str().unwrap_or_default()) {
            Some(captures) => match captures.get(1) {
                Some(version) => {
                    let version = version.as_str().to_string();
                    let mut symlinks = vec![symlink_resolved_python_exe.to_owned()];
                    for possible_symlink in [
                        PathBuf::from(format!("/opt/homebrew/bin/python{}", version)),
                        PathBuf::from(format!("/opt/homebrew/opt/python@{}/bin/python{}", version, version)),
                        PathBuf::from(format!("/opt/homebrew/Cellar/python@{}/{}/bin/python{}",version,  full_version, version)),
                        PathBuf::from(format!("/opt/homebrew/Cellar/python@{}/{}/Frameworks/Python.framework/Versions/{}/bin/python{}", version, full_version, version, version)),
                        PathBuf::from(format!("/opt/homebrew/Cellar/python@{}/{}/Frameworks/Python.framework/Versions/Current/bin/python{}", version, full_version, version)),
                        PathBuf::from(format!("/opt/homebrew/Frameworks/Python.framework/Versions/{}/bin/python{}", version, version)),
                        PathBuf::from(format!("/opt/homebrew/Frameworks/Python.framework/Versions/Current/bin/python{}", version)),
                        // Check if this symlink is pointing to the same place as the resolved python exe
                        PathBuf::from(format!("/opt/homebrew/opt/python3/bin/python{}", version)),
                        // Check if this symlink is pointing to the same place as the resolved python exe
                        PathBuf::from("/opt/homebrew/bin/python3"),
                        // Check if this symlink is pointing to the same place as the resolved python exe
                        PathBuf::from("/opt/homebrew/bin/python")
                        ] {

                        // Validate the symlinks
                        if symlinks.contains(
                            &resolve_symlink(&possible_symlink)
                                // .or(fs::canonicalize(&possible_symlink).ok())
                                .unwrap_or_default(),
                        ) {
                            symlinks.push(possible_symlink);
                        }
                    }

                    symlinks
                }
                None => vec![],
            },
            None => vec![],
        }
    } else if symlink_resolved_python_exe.starts_with("/usr/local/Cellar") {
        // Real exe - /usr/local/Cellar/python@3.8/3.8.19/Frameworks/Python.framework/Versions/3.8/bin/python3.8

        // Known symlinks include
        // /usr/local/bin/python3.8
        // /usr/local/opt/python@3.8/bin/python3.8
        // /usr/local/Cellar/python@3.8/3.8.19/bin/python3.8
        // /usr/local/Cellar/python@3.8/3.8.19/Frameworks/Python.framework/Versions/3.8/bin/python3.8
        match PYTHON_VERSION.captures(symlink_resolved_python_exe.to_str().unwrap_or_default()) {
            Some(captures) => match captures.get(1) {
                Some(version) => {
                    let version = version.as_str().to_string();
                    // Never include `/usr/local/bin/python` into this list.
                    // See previous explanation
                    let mut symlinks = vec![symlink_resolved_python_exe.to_owned()];
                    for possible_symlink in [
                            // While testing found that on Mac Intel
                            // 1. python 3.8 has sysprefix in /usr/local/Cellar/python@3.9/3.9.19/Frameworks/Python.framework/Versions/3.9
                            // 2. python 3.9 has sysprefix in /usr/local/opt/python@3.9/Frameworks/Python.framework/Versions/3.9
                            // 3. python 3.11 has sysprefix in /usr/local/opt/python@3.11/Frameworks/Python.framework/Versions/3.11
                            // Hence till we know more about it, this path is not included, but left as commented
                            // So we can add it later if needed & tested
                            // PathBuf::from(format!(
                            //     "/usr/local/opt/python@{}/bin/python{}",
                            //     version, version
                            // )),
                            PathBuf::from(format!(
                                "/usr/local/Cellar/python@{}/{}/bin/python{}",
                                version, full_version, version
                            )),
                            PathBuf::from(format!(
                                "/usr/local/Cellar/python@{}/{}/Frameworks/Python.framework/Versions/{}/bin/python{}",
                                version, full_version, version, version
                            )),
                            // This is a special folder, if users install python using other means, this file
                            // might get overridden. So we should only add this if this files points to the same place
                            PathBuf::from(format!("/usr/local/bin/python{}", version)),
                            // Check if this symlink is pointing to the same place as the resolved python exe
                            PathBuf::from("/usr/local/bin/python3"),
                            // Check if this symlink is pointing to the same place as the resolved python exe
                            PathBuf::from("/usr/local/bin/python"),
                        ] {

                        // Validate the symlinks
                        if symlinks.contains(
                            &resolve_symlink(&possible_symlink)
                                // .or(fs::canonicalize(&possible_symlink).ok())
                                .unwrap_or_default(),
                        ) {
                            symlinks.push(possible_symlink);
                        }
                    }

                    symlinks
                }
                None => vec![],
            },
            None => vec![],
        }
    } else if symlink_resolved_python_exe.starts_with("/home/linuxbrew/.linuxbrew/Cellar") {
        // Real exe - /home/linuxbrew/.linuxbrew/Cellar/python@3.12/3.12.3/bin/python3.12

        // Known symlinks include
        // /usr/local/bin/python3.12
        // /home/linuxbrew/.linuxbrew/bin/python3.12
        // /home/linuxbrew/.linuxbrew/opt/python@3.12/bin/python3.12
        match PYTHON_VERSION.captures(symlink_resolved_python_exe.to_str().unwrap_or_default()) {
            Some(captures) => match captures.get(1) {
                Some(version) => {
                    let version = version.as_str().to_string();
                    // Never include `/usr/local/bin/python` into this list.
                    // See previous explanation
                    let mut symlinks = vec![symlink_resolved_python_exe.to_owned()];
                    for possible_symlink in [
                        PathBuf::from(format!("/home/linuxbrew/.linuxbrew/bin/python{}", version)),
                        PathBuf::from(format!(
                            "/home/linuxbrew/.linuxbrew/opt/python@{}/bin/python{}",
                            version, version
                        )),
                        // This is a special folder, if users install python using other means, this file
                        // might get overridden. So we should only add this if this files points to the same place
                        PathBuf::from(format!("/usr/local/bin/python{}", version)),
                        // Check if this symlink is pointing to the same place as the resolved python exe
                        PathBuf::from("/usr/local/bin/python3"),
                        // Check if this symlink is pointing to the same place as the resolved python exe
                        PathBuf::from("/usr/local/bin/python"),
                    ] {
                        // Validate the symlinks
                        if symlinks.contains(
                            &resolve_symlink(&possible_symlink)
                                .or(fs::canonicalize(&possible_symlink).ok())
                                .unwrap_or_default(),
                        ) {
                            symlinks.push(possible_symlink);
                        }
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
