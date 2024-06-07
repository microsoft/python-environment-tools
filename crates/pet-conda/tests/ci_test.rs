// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::path::PathBuf;

use serde::Deserialize;

mod common;

#[cfg(unix)]
#[cfg_attr(feature = "ci_conda", test)]
#[allow(dead_code)]
// We should detect the conda install along with the base env
fn detect_conda_root() {
    use pet_conda::Conda;
    use pet_core::{
        manager::EnvManagerType, os_environment::EnvironmentApi,
        python_environment::PythonEnvironmentCategory, Locator,
    };
    use pet_reporter::test::create_reporter;

    let env = EnvironmentApi::new();

    let reporter = create_reporter();
    let conda = Conda::from(&env);
    conda.find(&reporter);
    let result = reporter.get_result();

    assert_eq!(result.managers.len(), 1);

    let info = get_conda_info();
    let conda_dir = PathBuf::from(info.conda_prefix.clone());
    let manager = &result.managers[0];
    assert_eq!(manager.executable, conda_dir.join("bin").join("conda"));
    assert_eq!(manager.tool, EnvManagerType::Conda);
    assert_eq!(manager.version, info.conda_version.into());

    let env = &result
        .environments
        .iter()
        .find(|e| e.name == Some("base".into()))
        .unwrap();
    assert_eq!(env.prefix, conda_dir.clone().into());
    assert_eq!(env.name, Some("base".into()));
    assert_eq!(env.category, PythonEnvironmentCategory::Conda);
    assert_eq!(env.executable, Some(conda_dir.join("bin").join("python")));
    assert_eq!(env.version, Some(get_version(&info.python_version)));

    assert_eq!(env.manager, Some(manager.clone()));
}

#[cfg(unix)]
#[cfg_attr(feature = "ci_conda", test)]
#[allow(dead_code)]
// Given the path to the root directory, detect the manager and base env
fn detect_conda_root_from_path() {
    use pet_conda::Conda;
    use pet_core::{
        manager::EnvManagerType, os_environment::EnvironmentApi,
        python_environment::PythonEnvironmentCategory, Locator,
    };
    use pet_utils::env::PythonEnv;
    use std::path::PathBuf;

    let env = EnvironmentApi::new();
    let info = get_conda_info();
    let conda_dir = PathBuf::from(info.conda_prefix.clone());
    let exe = conda_dir.join("bin").join("python");
    let conda = Conda::from(&env);

    let python_env = PythonEnv::new(exe, Some(conda_dir.clone()), None);
    let env = conda.from(&python_env).unwrap();

    assert_eq!(env.manager.is_some(), true);

    let manager = env.manager.unwrap();
    assert_eq!(manager.executable, conda_dir.join("bin").join("conda"));
    assert_eq!(manager.tool, EnvManagerType::Conda);
    assert_eq!(manager.version, info.conda_version.into());

    assert_eq!(env.prefix, conda_dir.clone().into());
    assert_eq!(env.name, Some("base".into()));
    assert_eq!(env.category, PythonEnvironmentCategory::Conda);
    assert_eq!(env.executable, Some(conda_dir.join("bin").join("python")));
    assert_eq!(env.version, Some(get_version(&info.python_version)));
}

#[cfg(unix)]
#[cfg_attr(feature = "ci_conda", test)]
#[allow(dead_code)]
// When a new env is created detect that too
fn detect_new_conda_env() {
    use pet_conda::Conda;
    use pet_core::{
        os_environment::EnvironmentApi, python_environment::PythonEnvironmentCategory, Locator,
    };
    use pet_reporter::test::create_reporter;
    use std::path::PathBuf;

    let env_name = "env_with_python";
    create_conda_env(
        &CondaCreateEnvNameOrPath::Name(env_name.into()),
        Some("3.10".into()),
    );
    let env = EnvironmentApi::new();

    let conda = Conda::from(&env);
    let reporter = create_reporter();
    conda.find(&reporter);
    let result = reporter.get_result();

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
#[cfg_attr(feature = "ci_conda", test)]
#[allow(dead_code)]
// Identify the manager and conda env given the path to a conda env inside the `envs` directory
fn detect_conda_env_from_path() {
    use pet_conda::Conda;
    use pet_core::{
        manager::EnvManagerType, os_environment::EnvironmentApi,
        python_environment::PythonEnvironmentCategory, Locator,
    };
    use pet_utils::env::PythonEnv;
    use std::path::PathBuf;

    let env = EnvironmentApi::new();
    let info = get_conda_info();
    let env_name = "env_with_python2";
    create_conda_env(
        &CondaCreateEnvNameOrPath::Name(env_name.into()),
        Some("3.10".into()),
    );
    let conda_dir = PathBuf::from(info.conda_prefix.clone());
    let prefix = conda_dir.join("envs").join(env_name);
    let exe = prefix.join("bin").join("python");
    let conda = Conda::from(&env);

    let python_env = PythonEnv::new(exe.clone(), Some(prefix.clone()), None);
    let env = conda.from(&python_env).unwrap();

    assert_eq!(env.manager.is_some(), true);

    let manager = env.manager.unwrap();
    assert_eq!(manager.executable, conda_dir.join("bin").join("conda"));
    assert_eq!(manager.tool, EnvManagerType::Conda);
    assert_eq!(manager.version, info.conda_version.into());

    assert_eq!(env.prefix, prefix.clone().into());
    assert_eq!(env.name, Some(env_name.into()));
    assert_eq!(env.category, PythonEnvironmentCategory::Conda);
    assert_eq!(env.executable, exe.clone().into());
    assert!(
        env.version.clone().unwrap_or_default().starts_with("3.10"),
        "Expected 3.10, but got Version: {:?}",
        env.version
    );
}

#[cfg(unix)]
#[cfg_attr(feature = "ci_conda", test)]
#[allow(dead_code)]
// Detect envs created without Python
fn detect_new_conda_env_without_python() {
    use pet_conda::Conda;
    use pet_core::{
        os_environment::EnvironmentApi, python_environment::PythonEnvironmentCategory, Locator,
    };
    use pet_reporter::test::create_reporter;
    use std::path::PathBuf;

    let env_name = "env_without_python";
    create_conda_env(&CondaCreateEnvNameOrPath::Name(env_name.into()), None);
    let env = EnvironmentApi::new();

    let conda = Conda::from(&env);
    let reporter = create_reporter();
    conda.find(&reporter);
    let result = reporter.get_result();

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

#[cfg(unix)]
#[cfg_attr(feature = "ci_conda", test)]
#[allow(dead_code)]
// Detect envs created without Python in a custom directory using the -p flag
fn detect_new_conda_env_created_with_p_flag_without_python() {
    use common::resolve_test_path;
    use pet_conda::Conda;
    use pet_core::{
        os_environment::EnvironmentApi, python_environment::PythonEnvironmentCategory, Locator,
    };
    use pet_reporter::test::create_reporter;
    use std::path::PathBuf;

    let env_name = "env_without_python3";
    let prefix = resolve_test_path(&["unix", env_name]);
    create_conda_env(&CondaCreateEnvNameOrPath::Path(prefix.clone()), None);
    let env = EnvironmentApi::new();

    let conda = Conda::from(&env);
    let reporter = create_reporter();
    conda.find(&reporter);
    let result = reporter.get_result();

    assert_eq!(result.managers.len(), 1);

    let manager = &result.managers[0];

    let info = get_conda_info();
    let conda_dir = PathBuf::from(info.conda_prefix.clone());
    let env = result
        .environments
        .iter()
        .find(|x| x.prefix == Some(prefix.clone()))
        .expect(
            format!(
                "New Environment not created, detected envs {:?}",
                result.environments
            )
            .as_str(),
        );

    let prefix = conda_dir.clone().join("envs").join(env_name);
    assert_eq!(env.prefix, prefix.clone().into());
    assert_eq!(env.name, None);
    assert_eq!(env.category, PythonEnvironmentCategory::Conda);
    assert_eq!(env.executable.is_none(), true);
    assert_eq!(env.version.is_none(), true);

    assert_eq!(env.manager, Some(manager.clone()));
}

#[cfg(unix)]
#[cfg_attr(feature = "ci_conda", test)]
#[allow(dead_code)]
// Detect envs created Python in a custom directory using the -p flag
fn detect_new_conda_env_created_with_p_flag_with_python() {
    use common::resolve_test_path;
    use pet_conda::Conda;
    use pet_core::{
        os_environment::EnvironmentApi, python_environment::PythonEnvironmentCategory, Locator,
    };
    use pet_reporter::test::create_reporter;
    use std::path::PathBuf;

    let env_name = "env_with_python3";
    let prefix = resolve_test_path(&["unix", env_name]);
    let exe = prefix.join("bin").join("python");
    create_conda_env(
        &CondaCreateEnvNameOrPath::Path(prefix.clone()),
        Some("3.10".into()),
    );
    let env = EnvironmentApi::new();

    let conda = Conda::from(&env);
    let reporter = create_reporter();
    conda.find(&reporter);
    let result = reporter.get_result();

    assert_eq!(result.managers.len(), 1);

    let manager = &result.managers[0];

    let info = get_conda_info();
    let conda_dir = PathBuf::from(info.conda_prefix.clone());
    let env = result
        .environments
        .iter()
        .find(|x| x.prefix == Some(prefix.clone()))
        .expect(
            format!(
                "New Environment not created, detected envs {:?}",
                result.environments
            )
            .as_str(),
        );

    let prefix = conda_dir.clone().join("envs").join(env_name);
    assert_eq!(env.prefix, prefix.clone().into());
    assert_eq!(env.name, None);
    assert_eq!(env.category, PythonEnvironmentCategory::Conda);
    assert_eq!(env.executable, exe.into());
    assert!(
        env.version.clone().unwrap_or_default().starts_with("3.10"),
        "Expected 3.10, but got Version: {:?}",
        env.version
    );

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

enum CondaCreateEnvNameOrPath {
    Name(String),
    Path(PathBuf),
}

fn create_conda_env(mode: &CondaCreateEnvNameOrPath, python_version: Option<String>) {
    let mut cli: Vec<String> = vec!["create".to_string()];
    match mode {
        CondaCreateEnvNameOrPath::Name(name) => {
            cli.push("-n".to_string());
            cli.push(name.to_string());
        }
        CondaCreateEnvNameOrPath::Path(path) => {
            cli.push("-p".to_string());
            cli.push(path.to_str().unwrap().to_string());
        }
    }
    if let Some(ref python_version) = python_version {
        cli.push(format!("python={}", python_version.as_str()));
    }
    cli.push("-y".to_string());
    // Spawn `conda --version` to get the version of conda as a string
    let _ = std::process::Command::new(get_conda_exe())
        .args(cli)
        .output()
        .expect("Failed to execute command");
}

fn get_version(value: &String) -> String {
    // Regex to extract just the d.d.d version from the full version string
    let re = regex::Regex::new(r"\d+\.\d+\.\d+").unwrap();
    let captures = re.captures(value).unwrap();
    captures.get(0).unwrap().as_str().to_string()
}
