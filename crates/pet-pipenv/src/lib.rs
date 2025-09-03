// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use env_variables::EnvVariables;
use pet_core::env::PythonEnv;
use pet_core::os_environment::Environment;
use pet_core::LocatorKind;
use pet_core::{
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind},
    reporter::Reporter,
    Locator,
};
use pet_fs::path::norm_case;
use pet_python_utils::executable::find_executables;
use pet_python_utils::version;
use std::path::Path;
use std::{fs, path::PathBuf};

mod env_variables;

fn get_pipenv_project(env: &PythonEnv) -> Option<PathBuf> {
    if let Some(prefix) = &env.prefix {
        if let Some(project) = get_pipenv_project_from_prefix(prefix) {
            return Some(project);
        }
    }

    // We can also have a venv in the workspace that has pipenv installed in it.
    // In such cases, the project is the workspace folder containing the venv.
    if let Some(project) = &env.project {
        if project.join("Pipfile").exists() {
            return Some(project.clone());
        }
    }

    // If the parent is bin or script, then get the parent.
    let bin = env.executable.parent()?;
    if bin.file_name().unwrap_or_default() == Path::new("bin")
        || bin.file_name().unwrap_or_default() == Path::new("Scripts")
    {
        get_pipenv_project_from_prefix(env.executable.parent()?.parent()?)
    } else {
        get_pipenv_project_from_prefix(env.executable.parent()?)
    }
}

fn get_pipenv_project_from_prefix(prefix: &Path) -> Option<PathBuf> {
    let project_file = prefix.join(".project");
    if !project_file.exists() {
        return None;
    }
    let contents = fs::read_to_string(project_file).ok()?;
    let project_folder = norm_case(PathBuf::from(contents.trim().to_string()));
    if project_folder.exists() {
        Some(project_folder)
    } else {
        None
    }
}

fn is_pipenv_from_project(env: &PythonEnv) -> bool {
    if let Some(project) = &env.project {
        if project.join("Pipfile").exists() {
            return true;
        }
    }
    false
}

fn is_pipenv(env: &PythonEnv, env_vars: &EnvVariables) -> bool {
    if let Some(project_path) = get_pipenv_project(env) {
        if project_path.join(env_vars.pipenv_pipfile.clone()).exists() {
            return true;
        }
    }
    if is_pipenv_from_project(env) {
        return true;
    }
    // If we have a Pipfile, then this is a pipenv environment.
    // Else likely a virtualenvwrapper or the like.
    if let Some(project_path) = get_pipenv_project(env) {
        project_path.join(env_vars.pipenv_pipfile.clone()).exists()
    } else {
        false
    }
}

pub struct PipEnv {
    env_vars: EnvVariables,
}

impl PipEnv {
    pub fn from(environment: &dyn Environment) -> PipEnv {
        PipEnv {
            env_vars: EnvVariables::from(environment),
        }
    }
}
impl Locator for PipEnv {
    fn get_kind(&self) -> LocatorKind {
        LocatorKind::PipEnv
    }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::Pipenv]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if !is_pipenv(env, &self.env_vars) {
            return None;
        }
        let project_path = get_pipenv_project(env)?;
        let mut prefix = env.prefix.clone();
        if prefix.is_none() {
            if let Some(bin) = env.executable.parent() {
                if bin.file_name().unwrap_or_default() == Path::new("bin")
                    || bin.file_name().unwrap_or_default() == Path::new("Scripts")
                {
                    if let Some(dir) = bin.parent() {
                        prefix = Some(dir.to_owned());
                    }
                }
            }
        }
        let bin = env.executable.parent()?;
        let symlinks = find_executables(bin);
        let mut version = env.version.clone();
        if version.is_none() && prefix.is_some() {
            if let Some(prefix) = &prefix {
                version = version::from_creator_for_virtual_env(prefix);
            }
        }
        Some(
            PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::Pipenv))
                .executable(Some(env.executable.clone()))
                .version(version)
                .prefix(prefix)
                .project(Some(project_path))
                .symlinks(Some(symlinks))
                .build(),
        )
    }

    fn find(&self, _reporter: &dyn Reporter) {
        //
    }
}
