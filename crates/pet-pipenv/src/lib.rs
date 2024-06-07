// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{fs, path::PathBuf};
use pet_core::{
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentCategory},
    reporter::Reporter,
    Locator,
};
use pet_utils::{env::PythonEnv, path::normalize};

fn get_pipenv_project(env: &PythonEnv) -> Option<PathBuf> {
    let project_file = env.prefix.clone()?.join(".project");
    let contents = fs::read_to_string(project_file).ok()?;
    let project_folder = normalize(PathBuf::from(contents.trim().to_string()));
    if fs::metadata(&project_folder).is_ok() {
        Some(project_folder)
    } else {
        None
    }
}

fn is_pipenv(env: &PythonEnv) -> bool {
    // If we have a Pipfile, then this is a pipenv environment.
    // Else likely a virtualenvwrapper or the like.
    if let Some(project_path) = get_pipenv_project(env) {
        fs::metadata(project_path.join("Pipfile")).is_ok()
    } else {
        false
    }
}

pub struct PipEnv {}

impl PipEnv {
    pub fn new() -> PipEnv {
        PipEnv {}
    }
}
impl Default for PipEnv {
    fn default() -> Self {
        Self::new()
    }
}
impl Locator for PipEnv {
    fn from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if !is_pipenv(env) {
            return None;
        }
        let project_path = get_pipenv_project(env)?;
        Some(
            PythonEnvironmentBuilder::new(PythonEnvironmentCategory::Pipenv)
                .executable(Some(env.executable.clone()))
                .version(env.version.clone())
                .prefix(env.prefix.clone())
                .project(Some(project_path))
                .build(),
        )
    }

    fn find(&self, _reporter: &dyn Reporter) {
        //
    }
}
