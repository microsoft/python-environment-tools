// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use log::warn;
use pet_core::arch::Architecture;
use regex::Regex;
use serde::Deserialize;
use std::fs::read_to_string;
use std::path::{Path, PathBuf};

lazy_static! {
    static ref PYTHON_VERSION: Regex = Regex::new("^python-((\\d+\\.*)*)-.*.json$")
        .expect("error parsing Version regex for Python Package Version in conda");
    static ref CONDA_VERSION: Regex = Regex::new("^conda-((\\d+\\.*)*)-.*.json$")
        .expect("error parsing Version regex for Conda Package Version in conda");
}

use std::{fmt, fs};

#[derive(Debug, Clone, PartialEq)]
pub enum Package {
    Conda,
    Python,
}

impl Package {
    pub fn to_name(&self) -> &str {
        match self {
            Package::Conda => "conda",
            Package::Python => "python",
        }
    }
}

impl fmt::Display for Package {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Package::Conda => write!(f, "Conda"),
            Package::Python => write!(f, "Python"),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct CondaPackageInfo {
    pub package: Package,
    #[allow(dead_code)]
    pub path: PathBuf,
    pub version: String,
    pub arch: Option<Architecture>,
}

impl CondaPackageInfo {
    pub fn from(path: &Path, package: &Package) -> Option<Self> {
        get_conda_package_info(path, package)
    }
}

#[derive(Deserialize, Debug)]
struct CondaMetaPackageStructure {
    channel: Option<String>,
    version: Option<String>,
}

/// Get the details of a conda package from the 'conda-meta' directory.
fn get_conda_package_info(path: &Path, name: &Package) -> Option<CondaPackageInfo> {
    // conda-meta is in the root of the conda installation folder
    let path = path.join("conda-meta");

    let history = path.join("history");
    let package_entry = format!(":{}", name.to_name());
    if let Some(history_contents) = fs::read_to_string(&history).ok() {
        for line in history_contents
            .lines()
            .filter(|l| l.contains(&package_entry))
        {
            // Sample entry in the history file
            // +conda-forge/osx-arm64::psutil-5.9.8-py312he37b823_0
            // +conda-forge/osx-arm64::python-3.12.2-hdf0ec26_0_cpython
            // +conda-forge/osx-arm64::python_abi-3.12-4_cp312
            if let Some(package_path) = line.split(&package_entry).nth(1) {
                let package_path = path.join(format!("{}{}.json", name.to_name(), package_path));
                let mut arch: Option<Architecture> = None;
                // Sample contents
                // {
                //   "build": "h966fe2a_2",
                //   "build_number": 2,
                //   "channel": "https://repo.anaconda.com/pkgs/main/win-64",
                //   "constrains": [],
                // }
                // 32bit channel is https://repo.anaconda.com/pkgs/main/win-32/
                // 64bit channel is "channel": "https://repo.anaconda.com/pkgs/main/osx-arm64",
                if let Some(contents) = read_to_string(&package_path).ok() {
                    if let Some(js) =
                        serde_json::from_str::<CondaMetaPackageStructure>(&contents).ok()
                    {
                        if let Some(channel) = js.channel {
                            if channel.ends_with("64") {
                                arch = Some(Architecture::X64);
                            } else if channel.ends_with("32") {
                                arch = Some(Architecture::X86);
                            }
                        }
                        if let Some(version) = js.version {
                            return Some(CondaPackageInfo {
                                package: name.clone(),
                                path: package_path,
                                version,
                                arch,
                            });
                        } else {
                            warn!(
                                "Unable to find version for package {} in {:?}",
                                name, package_path
                            );
                        }
                    }
                }
            }
        }
    }

    warn!(
        "Unable to find conda package {} in {:?}, trying slower approach",
        name, path
    );

    let package_name = format!("{}-", name.to_name());
    let regex = get_package_version_regex(&name);

    // Fallback, slower approach of enumerating all files.
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let file_name = path.file_name()?.to_string_lossy();
            if file_name.starts_with(&package_name) && file_name.ends_with(".json") {
                if let Some(captures) = regex.captures(&file_name) {
                    if let Some(version) = captures.get(1) {
                        let mut arch: Option<Architecture> = None;
                        // Sample contents
                        // {
                        //   "build": "h966fe2a_2",
                        //   "build_number": 2,
                        //   "channel": "https://repo.anaconda.com/pkgs/main/win-64",
                        //   "constrains": [],
                        // }
                        // 32bit channel is https://repo.anaconda.com/pkgs/main/win-32/
                        // 64bit channel is "channel": "https://repo.anaconda.com/pkgs/main/osx-arm64",
                        if let Some(contents) = read_to_string(&path).ok() {
                            if let Some(js) =
                                serde_json::from_str::<CondaMetaPackageStructure>(&contents).ok()
                            {
                                if let Some(channel) = js.channel {
                                    if channel.ends_with("64") {
                                        arch = Some(Architecture::X64);
                                    } else if channel.ends_with("32") {
                                        arch = Some(Architecture::X86);
                                    }
                                }
                            }
                        }
                        return Some(CondaPackageInfo {
                            package: name.clone(),
                            path: path.clone(),
                            version: version.as_str().to_string(),
                            arch,
                        });
                    }
                }
            }
        }
    }
    None
}

fn get_package_version_regex(package: &Package) -> &Regex {
    match package {
        Package::Conda => &CONDA_VERSION,
        Package::Python => &PYTHON_VERSION,
    }
}
