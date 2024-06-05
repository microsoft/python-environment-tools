// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use serde::Deserialize;

mod common;

#[cfg(unix)]
#[cfg_attr(feature = "ci", test)]
#[allow(dead_code)]
fn conda_root_ci() {
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
    assert_eq!(env.prefix, PathBuf::from(info.conda_prefix.clone()).into());
    assert_eq!(env.version.is_some(), true);
    assert_eq!(env.name, Some("base".into()));
    assert_eq!(env.category, PythonEnvironmentCategory::Conda);
    assert_eq!(env.executable, Some(conda_dir.join("bin").join("python")));

    assert_eq!(env.manager, Some(manager.clone()));
}

#[derive(Deserialize)]
struct CondaInfo {
    conda_version: String,
    conda_prefix: String,
    #[allow(dead_code)]
    envs: Vec<String>,
}

fn get_conda_info() -> CondaInfo {
    // On CI we expect conda to be in the current path.
    let conda_exe = "conda";
    // Spawn `conda --version` to get the version of conda as a string
    let output = std::process::Command::new(conda_exe)
        .args(["info", "--json"])
        .output()
        .expect("Failed to execute command");
    let output = String::from_utf8(output.stdout).unwrap();
    let conda_info: CondaInfo = serde_json::from_str(&output).unwrap();
    conda_info
}
