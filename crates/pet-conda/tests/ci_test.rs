// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use serde::Deserialize;

mod common;

#[cfg(unix)]
#[cfg_attr(feature = "ci", test)]
#[allow(dead_code)]
fn detect_conda_root() {
    use pet_conda::Conda;
    use pet_core::{
        manager::EnvManagerType, os_environment::EnvironmentApi,
        python_environment::PythonEnvironmentCategory, Locator,
    };
    use std::path::PathBuf;

    let env = EnvironmentApi::new();

    let conda = Conda::from(&env);
    let result = conda.find().unwrap();

    assert_eq!(result.managers.len(), 1);
    assert_eq!(result.environments.len(), 1);

    let info = get_conda_info();
    let conda_dir = PathBuf::from(info.conda_prefix.clone());
    let manager = &result.managers[0];
    assert_eq!(manager.executable, conda_dir.join("bin").join("conda"));
    assert_eq!(manager.tool, EnvManagerType::Conda);
    assert_eq!(manager.version, info.conda_version.into());

    let env = &result.environments[0];
    assert_eq!(env.prefix, conda_dir.clone().into());
    assert_eq!(env.name, Some("base".into()));
    assert_eq!(env.category, PythonEnvironmentCategory::Conda);
    assert_eq!(env.executable, Some(conda_dir.join("bin").join("python")));
    assert_eq!(env.version, Some(get_version(&info.python_version)));

    assert_eq!(env.manager, Some(manager.clone()));
}

#[cfg(unix)]
#[cfg_attr(feature = "ci", test)]
#[allow(dead_code)]
fn detect_new_conda_env() {
    use pet_conda::Conda;
    use pet_core::{
        os_environment::EnvironmentApi, python_environment::PythonEnvironmentCategory, Locator,
    };
    use std::path::PathBuf;

    let env_name = "env_with_python";
    create_conda_env_with_python(&env_name);
    let env = EnvironmentApi::new();

    let conda = Conda::from(&env);
    let result = conda.find().unwrap();
    println!("After creating Env {:?}", result);

    assert_eq!(result.managers.len(), 1);

    let manager = &result.managers[0];

    let info = get_conda_info();
    let conda_dir = PathBuf::from(info.conda_prefix.clone());
    let env = result
        .environments
        .iter()
        .find(|x| x.name == Some(env_name.into()))
        .expect(
            format!(
                "New Environment not created, detected envs {:?}",
                result.environments
            )
            .as_str(),
        );

    let prefix = conda_dir.clone().join("envs").join(env_name);
    assert_eq!(env.prefix, prefix.clone().into());
    assert_eq!(env.name, Some(env_name.into()));
    assert_eq!(env.category, PythonEnvironmentCategory::Conda);
    assert_eq!(env.executable, prefix.join("bin").join("python").into());
    assert!(
        env.version.clone().unwrap_or_default().starts_with("3.10"),
        "Expected 3.10, but got Version: {:?}",
        env.version
    );

    assert_eq!(env.manager, Some(manager.clone()));
}

#[cfg(unix)]
#[cfg_attr(feature = "ci", test)]
#[allow(dead_code)]
fn detect_new_conda_env_without_python() {
    use pet_conda::Conda;
    use pet_core::{
        os_environment::EnvironmentApi, python_environment::PythonEnvironmentCategory, Locator,
    };
    use std::path::PathBuf;

    let env_name = "env_without_python";
    create_conda_env_without_python(&env_name);
    let env = EnvironmentApi::new();

    let conda = Conda::from(&env);
    let result = conda.find().unwrap();
    println!("After creating Env {:?}", result);

    assert_eq!(result.managers.len(), 1);

    let manager = &result.managers[0];

    let info = get_conda_info();
    let conda_dir = PathBuf::from(info.conda_prefix.clone());
    let env = result
        .environments
        .iter()
        .find(|x| x.name == Some(env_name.into()))
        .expect(
            format!(
                "New Environment not created, detected envs {:?}",
                result.environments
            )
            .as_str(),
        );

    let prefix = conda_dir.clone().join("envs").join(env_name);
    assert_eq!(env.prefix, prefix.clone().into());
    assert_eq!(env.name, Some(env_name.into()));
    assert_eq!(env.category, PythonEnvironmentCategory::Conda);
    assert_eq!(env.executable.is_none(), true);
    assert_eq!(env.version.is_none(), true);

    assert_eq!(env.manager, Some(manager.clone()));
}

#[derive(Deserialize)]
struct CondaInfo {
    conda_version: String,
    conda_prefix: String,
    python_version: String,
    #[allow(dead_code)]
    envs: Vec<String>,
}

fn get_conda_exe() -> &'static str {
    // On CI we expect conda to be in the current path.
    "conda"
}

fn get_conda_info() -> CondaInfo {
    // Spawn `conda --version` to get the version of conda as a string
    let output = std::process::Command::new(get_conda_exe())
        .args(["info", "--json"])
        .output()
        .expect("Failed to execute command");
    let output = String::from_utf8(output.stdout).unwrap();
    let conda_info: CondaInfo = serde_json::from_str(&output).unwrap();
    conda_info
}

fn create_conda_env_with_python(name: &str) {
    // Spawn `conda --version` to get the version of conda as a string
    let _ = std::process::Command::new(get_conda_exe())
        .args(["create", "-n", name, "python=3.10", "-y"])
        .output()
        .expect("Failed to execute command");
}

fn create_conda_env_without_python(name: &str) {
    // Spawn `conda --version` to get the version of conda as a string
    let _ = std::process::Command::new(get_conda_exe())
        .args(["create", "-n", name, "-y"])
        .output()
        .expect("Failed to execute command");
}

fn get_version(value: &String) -> String {
    // Regex to extract just the d.d.d version from the full version string
    let re = regex::Regex::new(r"\d+\.\d+\.\d+").unwrap();
    let captures = re.captures(value).unwrap();
    captures.get(0).unwrap().as_str().to_string()
}
