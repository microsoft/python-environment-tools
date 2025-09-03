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
        // If there's no .project file, but the venv lives inside the project folder
        // (e.g., <project>/.venv or <project>/venv), then the project is the parent
        // directory of the venv. Detect that by checking for a Pipfile next to the venv.
        if let Some(parent) = prefix.parent() {
            let project_folder = parent;
            if project_folder.join("Pipfile").exists() {
                return Some(project_folder.to_path_buf());
            }
        }
    }

    // We can also have a venv in the workspace that has pipenv installed in it.
    // In such cases, the project is the workspace folder containing the venv.
    // Derive the project folder from the executable path when prefix isn't available.
    // Typical layout: <project>/.venv/{bin|Scripts}/python
    // So walk up to {bin|Scripts} -> venv dir -> project dir and check for Pipfile.
    if let Some(bin) = env.executable.parent() {
        let venv_dir = if bin.file_name().unwrap_or_default() == Path::new("bin")
            || bin.file_name().unwrap_or_default() == Path::new("Scripts")
        {
            bin.parent()
        } else {
            Some(bin)
        };
        if let Some(venv_dir) = venv_dir {
            if let Some(project_dir) = venv_dir.parent() {
                if project_dir.join("Pipfile").exists() {
                    return Some(project_dir.to_path_buf());
                }
            }
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
    // If the env prefix is inside a project folder, check that folder for a Pipfile.
    if let Some(prefix) = &env.prefix {
        if let Some(project_dir) = prefix.parent() {
            if project_dir.join("Pipfile").exists() {
                return true;
            }
        }
    }
    // Derive from the executable path as a fallback.
    if let Some(bin) = env.executable.parent() {
        let venv_dir = if bin.file_name().unwrap_or_default() == Path::new("bin")
            || bin.file_name().unwrap_or_default() == Path::new("Scripts")
        {
            bin.parent()
        } else {
            Some(bin)
        };
        if let Some(venv_dir) = venv_dir {
            if let Some(project_dir) = venv_dir.parent() {
                if project_dir.join("Pipfile").exists() {
                    return true;
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir() -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.push(format!("pet_pipenv_test_{}", nanos));
        dir
    }

    #[test]
    fn infer_project_for_venv_in_project() {
        let project_dir = unique_temp_dir();
        let venv_dir = project_dir.join(".venv");
        let bin_dir = if cfg!(windows) {
            venv_dir.join("Scripts")
        } else {
            venv_dir.join("bin")
        };
        let python_exe = if cfg!(windows) {
            bin_dir.join("python.exe")
        } else {
            bin_dir.join("python")
        };

        // Create directories and files
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(project_dir.join("Pipfile"), b"[[source]]\n").unwrap();
        // Touch python exe file
        std::fs::write(&python_exe, b"").unwrap();
        // Touch pyvenv.cfg in venv root so PythonEnv::new logic would normally detect prefix
        std::fs::write(venv_dir.join("pyvenv.cfg"), b"version = 3.12.0\n").unwrap();

        // Construct PythonEnv directly
        let env = PythonEnv {
            executable: norm_case(python_exe.clone()),
            prefix: Some(norm_case(venv_dir.clone())),
            version: None,
            symlinks: None,
        };

        // Validate helper infers project
        let inferred = get_pipenv_project(&env).expect("expected project path");
        assert_eq!(inferred, norm_case(project_dir.clone()));

        // Validate locator populates project
        let locator = PipEnv {
            env_vars: EnvVariables {
                pipenv_max_depth: 3,
                pipenv_pipfile: "Pipfile".to_string(),
            },
        };
        let result = locator
            .try_from(&env)
            .expect("expected locator to return environment");
        assert_eq!(result.project, Some(norm_case(project_dir.clone())));

        // Cleanup
        std::fs::remove_dir_all(&project_dir).ok();
    }
}
