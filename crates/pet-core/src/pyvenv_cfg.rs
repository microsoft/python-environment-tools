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
    pub prompt: Option<String>,
}

impl PyVenvCfg {
    fn new(
        version: String,
        version_major: u64,
        version_minor: u64,
        prompt: Option<String>,
    ) -> Self {
        Self {
            version,
            version_major,
            version_minor,
            prompt,
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
    let mut version: Option<String> = None;
    let mut version_major: Option<u64> = None;
    let mut version_minor: Option<u64> = None;
    let mut prompt: Option<String> = None;

    for line in contents.lines() {
        if version.is_none() {
            if let Some((ver, major, minor)) = parse_version(line, &VERSION) {
                version = Some(ver);
                version_major = Some(major);
                version_minor = Some(minor);
                continue;
            }
            if let Some((ver, major, minor)) = parse_version(line, &VERSION_INFO) {
                version = Some(ver);
                version_major = Some(major);
                version_minor = Some(minor);
                continue;
            }
        }
        if prompt.is_none() {
            if let Some(p) = parse_prompt(line) {
                prompt = Some(p);
            }
        }
        if version.is_some() && prompt.is_some() {
            break;
        }
    }

    match (version, version_major, version_minor) {
        (Some(ver), Some(major), Some(minor)) => Some(PyVenvCfg::new(ver, major, minor, prompt)),
        _ => None,
    }
}

fn parse_version(line: &str, regex: &Regex) -> Option<(String, u64, u64)> {
    if let Some(captures) = regex.captures(line) {
        if let Some(value) = captures.get(1) {
            let version = value.as_str();
            let parts: Vec<&str> = version.split('.').collect();
            if parts.len() >= 2 {
                let version_major = parts[0]
                    .parse()
                    .expect("python major version to be an integer");
                let version_minor = parts[1]
                    .parse()
                    .expect("python minor version to be an integer");
                return Some((version.to_string(), version_major, version_minor));
            }
        }
    }
    None
}

fn parse_prompt(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with("prompt") {
        if let Some(eq_idx) = trimmed.find('=') {
            // let value = trimmed[eq_idx + 1..].trim();
            let mut name = trimmed[eq_idx + 1..].trim().to_string();
            // Strip any leading or trailing single or double quotes
            if name.starts_with('"') {
                name = name.trim_start_matches('"').to_string();
            }
            if name.ends_with('"') {
                name = name.trim_end_matches('"').to_string();
            }
            // Strip any leading or trailing single or double quotes
            if name.starts_with('\'') {
                name = name.trim_start_matches('\'').to_string();
            }
            if name.ends_with('\'') {
                name = name.trim_end_matches('\'').to_string();
            }
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}
