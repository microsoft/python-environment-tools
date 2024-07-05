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
}

impl PyVenvCfg {
    fn new(version: String) -> Self {
        Self { version }
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
        if let Some(captures) = VERSION.captures(line) {
            if let Some(value) = captures.get(1) {
                return Some(PyVenvCfg::new(value.as_str().to_string()));
            }
        }
        if let Some(captures) = VERSION_INFO.captures(line) {
            if let Some(value) = captures.get(1) {
                return Some(PyVenvCfg::new(value.as_str().to_string()));
            }
        }
    }
    None
}
