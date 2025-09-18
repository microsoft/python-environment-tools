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
    pub uv_version: Option<String>,
}

impl PyVenvCfg {
    fn new(
        version: String,
        version_major: u64,
        version_minor: u64,
        prompt: Option<String>,
        uv_version: Option<String>,
    ) -> Self {
        Self {
            version,
            version_major,
            version_minor,
            prompt,
            uv_version,
        }
    }
    pub fn is_uv(&self) -> bool {
        self.uv_version.is_some()
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

    if cfg!(windows) {
        // Only windows installations have a `Scripts` directory.
        if path.ends_with("Scripts") {
            let cfg = path.parent()?.join(PYVENV_CONFIG_FILE);
            if cfg.exists() {
                return Some(cfg);
            }
        }
    }
    // Some windows installations have a `bin` directory. https://github.com/microsoft/vscode-python/issues/24792
    if path.ends_with("bin") {
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
    let mut uv_version: Option<String> = None;

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
        if uv_version.is_none() {
            if let Some(uv_ver) = parse_uv_version(line) {
                uv_version = Some(uv_ver);
            }
        }
        if version.is_some() && prompt.is_some() && uv_version.is_some() {
            break;
        }
    }

    match (version, version_major, version_minor) {
        (Some(ver), Some(major), Some(minor)) => Some(PyVenvCfg::new(ver, major, minor, prompt, uv_version)),
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

fn parse_uv_version(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with("uv") {
        if let Some(eq_idx) = trimmed.find('=') {
            let mut version = trimmed[eq_idx + 1..].trim().to_string();
            // Strip any leading or trailing single or double quotes
            if version.starts_with('"') {
                version = version.trim_start_matches('"').to_string();
            }
            if version.ends_with('"') {
                version = version.trim_end_matches('"').to_string();
            }
            if version.starts_with('\'') {
                version = version.trim_start_matches('\'').to_string();
            }
            if version.ends_with('\'') {
                version = version.trim_end_matches('\'').to_string();
            }
            if !version.is_empty() {
                return Some(version);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, fs};

    #[test]
    fn test_parse_uv_version() {
        assert_eq!(parse_uv_version("uv = 0.8.14"), Some("0.8.14".to_string()));
        assert_eq!(parse_uv_version("uv=0.8.14"), Some("0.8.14".to_string()));
        assert_eq!(parse_uv_version("uv = \"0.8.14\""), Some("0.8.14".to_string()));
        assert_eq!(parse_uv_version("uv = '0.8.14'"), Some("0.8.14".to_string()));
        assert_eq!(parse_uv_version("version = 3.12.11"), None);
        assert_eq!(parse_uv_version("prompt = test-env"), None);
    }

    #[test]
    fn test_pyvenv_cfg_detects_uv() {
        let temp_file = "/tmp/test_pyvenv_uv.cfg";
        let contents = "home = /usr/bin/python3.12\nimplementation = CPython\nuv = 0.8.14\nversion_info = 3.12.11\ninclude-system-site-packages = false\nprompt = test-uv-env\n";
        fs::write(temp_file, contents).unwrap();
        
        let cfg = parse(&PathBuf::from(temp_file)).unwrap();
        assert!(cfg.is_uv());
        assert_eq!(cfg.uv_version, Some("0.8.14".to_string()));
        assert_eq!(cfg.prompt, Some("test-uv-env".to_string()));
        
        fs::remove_file(temp_file).ok();
    }

    #[test]
    fn test_pyvenv_cfg_regular_venv() {
        let temp_file = "/tmp/test_pyvenv_regular.cfg";
        let contents = "home = /usr/bin/python3.12\ninclude-system-site-packages = false\nversion = 3.13.5\nexecutable = /usr/bin/python3.12\ncommand = python -m venv /path/to/env\n";
        fs::write(temp_file, contents).unwrap();
        
        let cfg = parse(&PathBuf::from(temp_file)).unwrap();
        assert!(!cfg.is_uv());
        assert_eq!(cfg.uv_version, None);
        
        fs::remove_file(temp_file).ok();
    }
}
