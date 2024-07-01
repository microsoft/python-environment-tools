// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::trace;
use pet_core::manager::{EnvManager, EnvManagerType};
use std::{env, path::PathBuf};

use crate::env_variables::EnvVariables;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PoetryManager {
    pub executable: PathBuf,
}

impl PoetryManager {
    pub fn find(executable: Option<PathBuf>, env_variables: &EnvVariables) -> Option<Self> {
        if let Some(executable) = executable {
            if executable.is_file() {
                return Some(PoetryManager { executable });
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
                    return Some(PoetryManager { executable });
                }
            }

            // Look for poetry in current PATH.
            if let Some(env_path) = &env_variables.path {
                for each in env::split_paths(env_path) {
                    let executable = each.join("poetry");
                    if executable.is_file() {
                        return Some(PoetryManager { executable });
                    }
                    if std::env::consts::OS == "windows" {
                        let executable = each.join("poetry.exe");
                        if executable.is_file() {
                            return Some(PoetryManager { executable });
                        }
                    }
                }
            }
        }
        trace!("Poetry exe not found");
        None
    }
    pub fn to_manager(&self) -> EnvManager {
        EnvManager {
            executable: self.executable.clone(),
            version: None,
            tool: EnvManagerType::Poetry,
        }
    }
}
