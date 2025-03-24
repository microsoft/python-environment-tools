// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use regex::Regex;
use std::{
    fs,
    path::{Path, PathBuf},
};

lazy_static! {
    static ref VERSION: Regex = Regex::new(r"^version\s*=\s*(\d+\.\d+\.\d+)$")
        .expect("error parsing Version regex for pyenv_cfg");
    static ref VERSION_INFO: Regex = Regex::new(r"^version_info\s*=\s*(\d+\.\d+\.\d+.*)$")
        .expect("error parsing Version_info regex for pyenv_cfg");
}

const PYVENV_CONFIG_FILE: &str = "pyvenv.cfg";

#[derive(Debug)]
pub struct PyVenvCfg {
    pub version: String,
    pub version_major: u64,
    pub version_minor: u64,
}

impl PyVenvCfg {
    fn new(version: String, version_major: u64, version_minor: u64) -> Self {
        Self {
            version,
            version_major,
            version_minor,
        }
    }
    pub fn find(path: &Path) -> Option<Self> {
        if let Some(ref file) = find(path) {
            parse(file)
        } else {
            None
        }
    }
}

fn find(path: &Path) -> Option<PathBuf> {
    // env
    // |__ pyvenv.cfg  <--- check if this file exists
    // |__ bin or Scripts
    //     |__ python  <--- interpreterPath

    // Check if the pyvenv.cfg file is in the current directory.
    // Possible the passed value is the `env`` directory.
    let cfg = path.join(PYVENV_CONFIG_FILE);
    if cfg.exists() {
        return Some(cfg);
    }

    let bin = if cfg!(windows) { "Scripts" } else { "bin" };
    if path.ends_with(bin) {
        let cfg = path.parent()?.join(PYVENV_CONFIG_FILE);
        if cfg.exists() {
            return Some(cfg);
        }
    }
    // let cfg = path.parent()?.join(PYVENV_CONFIG_FILE);
    // println!("{:?}", cfg);
    // if fs::metadata(&cfg).is_ok() {
    //     return Some(cfg);
    // }

    // // Check if the pyvenv.cfg file is in the parent directory.
    // // Possible the passed value is the `bin` directory.
    // let cfg = path.parent()?.parent()?.join(PYVENV_CONFIG_FILE);
    // if fs::metadata(&cfg).is_ok() {
    //     return Some(cfg);
    // }

    None
}

fn parse(file: &Path) -> Option<PyVenvCfg> {
    let contents = fs::read_to_string(file).ok()?;
    for line in contents.lines() {
        if !line.contains("version") {
            continue;
        }
        if let Some(cfg) = parse_version(line, &VERSION) {
            return Some(cfg);
        }
        if let Some(cfg) = parse_version(line, &VERSION_INFO) {
            return Some(cfg);
        }
    }
    None
}

fn parse_version(line: &str, regex: &Regex) -> Option<PyVenvCfg> {
    if let Some(captures) = regex.captures(line) {
        if let Some(value) = captures.get(1) {
            let version = value.as_str();
            let parts: Vec<&str> = version.splitn(3, ".").take(2).collect();
            // .expect() below is OK because the version regex
            // guarantees there are at least two digits.
            let version_major = parts[0]
                .parse()
                .expect("python major version to be an integer");
            let version_minor = parts[1]
                .parse()
                .expect("python minor version to be an integer");
            return Some(PyVenvCfg::new(
                version.to_string(),
                version_major,
                version_minor,
            ));
        }
    }

    None
}
