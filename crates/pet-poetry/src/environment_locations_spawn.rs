// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use log::{error, trace};
use pet_core::python_environment::PythonEnvironment;
use regex::Regex;
use std::{path::PathBuf, time::SystemTime};

use crate::{environment::create_poetry_env, manager::PoetryManager};

lazy_static! {
    static ref SANITIZE_NAME: Regex = Regex::new("[ $`!*@\"\\\r\n\t]")
        .expect("Error generating RegEx for poetry file path hash generator");
}

pub fn list_environments(
    executable: &PathBuf,
    project_dirs: Vec<PathBuf>,
    manager: &PoetryManager,
) -> Vec<PythonEnvironment> {
    let mut envs = vec![];
    for project_dir in project_dirs {
        if let Some(project_envs) = get_environments(executable, &project_dir) {
            for project_env in project_envs {
                if let Some(env) =
                    create_poetry_env(&project_env, project_dir.clone(), Some(manager.clone()))
                {
                    envs.push(env);
                }
            }
        }
    }
    envs
}

fn get_environments(executable: &PathBuf, project_dir: &PathBuf) -> Option<Vec<PathBuf>> {
    let start = SystemTime::now();
    let result = std::process::Command::new(executable)
        .arg("env")
        .arg("list")
        .arg("--full-path")
        .current_dir(project_dir)
        .output();
    trace!(
        "Executed Poetry ({}ms): {:?} env list --full-path for {:?}",
        start.elapsed().unwrap_or_default().as_millis(),
        executable,
        project_dir
    );
    match result {
        Ok(output) => {
            if output.status.success() {
                let output = String::from_utf8_lossy(&output.stdout).to_string();
                Some(
                    output
                        .lines()
                        .map(|line|
                        // Remove the '(Activated)` suffix from the line
                        line.trim_end_matches(" (Activated)").trim())
                        .filter(|line| !line.is_empty())
                        .map(|line|
                        // Remove the '(Activated)` suffix from the line
                        PathBuf::from(line.trim_end_matches(" (Activated)").trim()))
                        .collect::<Vec<PathBuf>>(),
                )
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                trace!(
                    "Failed to get Poetry Envs using exe {:?} ({:?}) {}",
                    executable,
                    output.status.code().unwrap_or_default(),
                    stderr
                );
                None
            }
        }
        Err(err) => {
            error!("Failed to execute Poetry env list {:?}", err);
            None
        }
    }
}
