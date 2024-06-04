// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::env_variables::EnvVariables;
use lazy_static::lazy_static;
use regex::Regex;
use std::{fs, path::PathBuf};

lazy_static! {
    static ref PYTHON_VERSION: Regex =
        Regex::new(r"/(\d+\.\d+\.\d+)/").expect("error parsing Version regex for Homebrew");
}

// fn get_homebrew_prefix_env_var(env_vars: &EnvVariables) -> Option<PathBuf> {
//     if let Some(homebrew_prefix) = &env_vars.homebrew_prefix {
//         let homebrew_prefix_bin = PathBuf::from(homebrew_prefix).join("bin");
//         if fs::metadata(&homebrew_prefix_bin).is_ok() {
//             return Some(homebrew_prefix_bin);
//         }
//     }
//     None
// }

pub fn get_homebrew_prefix_bin(env_vars: &EnvVariables) -> Vec<PathBuf> {
    // Homebrew install folders documented here https://docs.brew.sh/Installation
    // /opt/homebrew for Apple Silicon,
    // /usr/local for macOS Intel
    // /home/linuxbrew/.linuxbrew for Linux
    // If user has rosetta enabled, then its possible we have homebrew installed via rosetta as well as apple silicon
    // I.e. we can have multiple home brews on the same machine, hence search all,
    let mut homebrew_prefixes = [
        "/home/linuxbrew/.linuxbrew/bin",
        "/opt/homebrew/bin",
        "/usr/local/bin",
    ]
    .iter()
    .map(PathBuf::from)
    .filter(|p| p.exists())
    .collect::<Vec<PathBuf>>();

    // Check the environment variables
    if let Some(homebrew_prefix) = &env_vars.homebrew_prefix {
        let homebrew_prefix_bin = PathBuf::from(homebrew_prefix).join("bin");
        if fs::metadata(&homebrew_prefix_bin).is_ok()
            && !homebrew_prefixes.contains(&homebrew_prefix_bin)
        {
            homebrew_prefixes.push(homebrew_prefix_bin);
        }
    }

    homebrew_prefixes
}
