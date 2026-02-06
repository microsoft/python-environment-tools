// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use log::trace;
use pet_core::manager::{EnvManager, EnvManagerType};
use pet_fs::path::resolve_any_symlink;
use regex::Regex;
use std::{env, path::PathBuf};

use crate::env_variables::EnvVariables;

lazy_static! {
    /// Matches Homebrew Cellar path for poetry: /Cellar/poetry/X.Y.Z or /Cellar/poetry/X.Y.Z_N
    static ref HOMEBREW_POETRY_VERSION: Regex =
        Regex::new(r"/Cellar/poetry/(\d+\.\d+\.\d+)").expect("error parsing Homebrew poetry version regex");
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PoetryManager {
    pub executable: PathBuf,
    pub version: Option<String>,
}

impl PoetryManager {
    pub fn find(executable: Option<PathBuf>, env_variables: &EnvVariables) -> Option<Self> {
        if let Some(executable) = executable {
            if executable.is_file() {
                let version = Self::extract_version_from_path(&executable);
                return Some(PoetryManager { executable, version });
            }
        }

        // Search in <home>/.poetry/bin/python (as done in Python Extension)

        if let Some(home) = &env_variables.home {
            let mut search_paths = vec![
                home.join(".poetry").join("bin").join("poetry"),
                // Found after installing on Mac using pipx
                home.join(".local")
                    .join("pipx")
                    .join("venvs")
                    .join("poetry")
                    .join("bin")
                    .join("poetry"),
            ];
            if let Some(poetry_home) = &env_variables.poetry_home {
                if std::env::consts::OS == "windows" {
                    search_paths.push(poetry_home.join("bin").join("poetry.exe"));
                    search_paths.push(poetry_home.join("venv").join("bin").join("poetry.exe"));
                }
                search_paths.push(poetry_home.join("bin").join("poetry"));
                search_paths.push(poetry_home.join("venv").join("bin").join("poetry"));
            }
            if std::env::consts::OS == "windows" {
                if let Some(app_data) = env_variables.app_data.clone() {
                    search_paths.push(
                        // https://python-poetry.org/docs/#installing-with-the-official-installer
                        app_data
                            .join("pypoetry")
                            .join("venv")
                            .join("Scripts")
                            .join("poetry.exe"),
                    );
                    search_paths.push(
                        // Found after installing on windows using Poetry install notes
                        app_data
                            .join("Roaming")
                            .join("Python")
                            .join("Scripts")
                            .join("poetry.exe"),
                    );
                    search_paths.push(
                        // https://python-poetry.org/docs/#installing-with-the-official-installer
                        app_data
                            .join("pypoetry")
                            .join("venv")
                            .join("Scripts")
                            .join("poetry"),
                    );
                    search_paths.push(
                        app_data.join("Python").join("scripts").join("poetry.exe"), // https://python-poetry.org/docs/#installing-with-the-official-installer
                    );
                    search_paths.push(
                        app_data.join("Python").join("scripts").join("poetry"), // https://python-poetry.org/docs/#installing-with-the-official-installer
                    );
                }
                search_paths.push(
                    // Found after installing on Windows via github actions.
                    home.join(".local").join("bin").join("poetry"),
                );
            } else if std::env::consts::OS == "macos" {
                search_paths.push(
                    // https://python-poetry.org/docs/#installing-with-the-official-installer
                    home.join("Library")
                        .join("Application Support")
                        .join("pypoetry")
                        .join("venv")
                        .join("bin")
                        .join("poetry"),
                );
                search_paths.push(
                    home.join(".local").join("bin").join("poetry"), // https://python-poetry.org/docs/#installing-with-the-official-installer
                );
            } else {
                search_paths.push(
                    // https://python-poetry.org/docs/#installing-with-the-official-installer
                    home.join(".local")
                        .join("share")
                        .join("pypoetry")
                        .join("venv")
                        .join("bin")
                        .join("poetry"),
                );
                search_paths.push(
                    home.join(".local").join("bin").join("poetry"), // https://python-poetry.org/docs/#installing-with-the-official-installer
                );
            }
            for executable in search_paths {
                if executable.is_file() {
                    let version = Self::extract_version_from_path(&executable);
                    return Some(PoetryManager { executable, version });
                }
            }

            // Look for poetry in current PATH.
            if let Some(env_path) = &env_variables.path {
                for each in env::split_paths(env_path) {
                    let executable = each.join("poetry");
                    if executable.is_file() {
                        let version = Self::extract_version_from_path(&executable);
                        return Some(PoetryManager { executable, version });
                    }
                    if std::env::consts::OS == "windows" {
                        let executable = each.join("poetry.exe");
                        if executable.is_file() {
                            let version = Self::extract_version_from_path(&executable);
                            return Some(PoetryManager { executable, version });
                        }
                    }
                }
            }
        }
        trace!("Poetry exe not found");
        None
    }

    /// Extracts poetry version from Homebrew Cellar path.
    ///
    /// Homebrew installs poetry to paths like:
    /// - macOS ARM: /opt/homebrew/Cellar/poetry/1.8.3_2/bin/poetry
    /// - macOS Intel: /usr/local/Cellar/poetry/1.8.3/bin/poetry
    /// - Linux: /home/linuxbrew/.linuxbrew/Cellar/poetry/1.8.3/bin/poetry
    ///
    /// The symlink at /opt/homebrew/bin/poetry points to the Cellar path.
    fn extract_version_from_path(executable: &PathBuf) -> Option<String> {
        // First try to resolve the symlink to get the actual Cellar path
        let resolved = resolve_any_symlink(executable).unwrap_or_else(|| executable.clone());
        let path_str = resolved.to_string_lossy();

        // Check if this is a Homebrew Cellar path and extract version
        if let Some(captures) = HOMEBREW_POETRY_VERSION.captures(&path_str) {
            if let Some(version_match) = captures.get(1) {
                let version = version_match.as_str().to_string();
                trace!("Extracted Poetry version {} from Homebrew path: {:?}", version, resolved);
                return Some(version);
            }
        }
        None
    }

    pub fn to_manager(&self) -> EnvManager {
        EnvManager {
            executable: self.executable.clone(),
            version: self.version.clone(),
            tool: EnvManagerType::Poetry,
        }
    }
}
