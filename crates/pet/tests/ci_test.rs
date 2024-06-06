// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use common::resolve_test_path;
use lazy_static::lazy_static;
use pet_core::{
    arch::Architecture,
    python_environment::{PythonEnvironment, PythonEnvironmentCategory},
};
use regex::Regex;
use serde::Deserialize;

lazy_static! {
    static ref PYTHON_VERSION: Regex = Regex::new("([\\d+\\.?]*).*")
        .expect("error parsing Version regex for Python Version in test");
}

mod common;

#[cfg(unix)]
#[cfg_attr(feature = "ci", test)]
#[allow(dead_code)]
// We should detect the conda install along with the base env
fn verify_validity_of_discovered_envs() {
    use std::thread;

    use pet::locators;
    use pet_reporter::{stdio, test};

    stdio::initialize_logger(log::LevelFilter::Warn);
    let reporter = test::create_reporter();
    locators::find_and_report_envs(&reporter);

    let environments = reporter
        .reported_environments
        .lock()
        .unwrap()
        .clone()
        .into_values()
        .collect::<Vec<_>>();
    let mut threads = vec![];
    for environment in environments {
        if environment.executable.is_none() {
            continue;
        }
        threads.push(thread::spawn(move || {
            verify_validity_of_interpreter_info(environment.clone());
        }));
    }
    for thread in threads {
        thread.join().unwrap();
    }
}

fn verify_validity_of_interpreter_info(environment: PythonEnvironment) {
    let run_command = get_python_run_command(&environment);
    let interpreter_info = get_python_interpreter_info(&run_command);

    // Home brew has too many syminks, unfortunately its not easy to test in CI.
    if environment.category != PythonEnvironmentCategory::Homebrew {
        let expected_executable = environment.executable.clone().unwrap();
        assert_eq!(
            expected_executable.to_str().unwrap(),
            interpreter_info.clone().executable,
            "Executable mismatch for {:?}",
            environment.clone()
        );
    }
    if let Some(prefix) = environment.clone().prefix {
        assert_eq!(
            prefix.to_str().unwrap(),
            interpreter_info.clone().sysPrefix,
            "Prefix mismatch for {:?}",
            environment.clone()
        );
    }
    if let Some(arch) = environment.clone().arch {
        let expected_arch = if interpreter_info.clone().is64Bit {
            Architecture::X64
        } else {
            Architecture::X86
        };
        assert_eq!(
            arch,
            expected_arch,
            "Architecture mismatch for {:?}",
            environment.clone()
        );
    }
    if let Some(version) = environment.clone().version {
        let expected_version = &interpreter_info.clone().sysVersion;
        let version = get_version(&version);
        assert!(
            expected_version.starts_with(&version),
            "Version mismatch for (expected {:?} to start with {:?}) for {:?}",
            expected_version,
            version,
            environment.clone()
        );
    }
}

#[allow(dead_code)]
fn get_conda_exe() -> &'static str {
    // On CI we expect conda to be in the current path.
    "conda"
}

#[derive(Deserialize, Clone)]
struct InterpreterInfo {
    #[allow(non_snake_case)]
    sysPrefix: String,
    executable: String,
    #[allow(non_snake_case)]
    sysVersion: String,
    #[allow(non_snake_case)]
    is64Bit: bool,
    // #[allow(non_snake_case)]
    // versionInfo: (u16, u16, u16, String, u16),
}

fn get_python_run_command(env: &PythonEnvironment) -> Vec<String> {
    if env.clone().category == PythonEnvironmentCategory::Conda {
        if env.executable.is_none() {
            panic!("Conda environment without executable");
        }
        let conda_exe = match env.manager.clone() {
            Some(manager) => manager.executable.to_str().unwrap_or_default().to_string(),
            None => get_conda_exe().to_string(),
        };
        if let Some(name) = env.name.clone() {
            return vec![
                conda_exe,
                "run".to_string(),
                "-n".to_string(),
                name,
                "python".to_string(),
            ];
        } else if let Some(prefix) = env.prefix.clone() {
            return vec![
                conda_exe,
                "run".to_string(),
                "-p".to_string(),
                prefix.to_str().unwrap_or_default().to_string(),
                "python".to_string(),
            ];
        } else {
            panic!("Conda environment without name or prefix")
        }
    } else {
        vec![env
            .executable
            .clone()
            .expect("Python environment without executable")
            .to_str()
            .unwrap()
            .to_string()]
    }
}

fn get_python_interpreter_info(cli: &Vec<String>) -> InterpreterInfo {
    let mut cli = cli.clone();
    cli.push(
        resolve_test_path(&["interpreterInfo.py"])
            .to_str()
            .unwrap_or_default()
            .to_string(),
    );
    // Spawn `conda --version` to get the version of conda as a string
    let output = std::process::Command::new(cli.first().unwrap())
        .args(&cli[1..])
        .output()
        .expect(format!("Failed to execute command {:?}", cli).as_str());
    let output = String::from_utf8(output.stdout).unwrap();
    let output = output
        .split_once("503bebe7-c838-4cea-a1bc-0f2963bcb657")
        .unwrap()
        .1;
    let info: InterpreterInfo = serde_json::from_str(&output).unwrap();
    info
}

fn get_version(value: &String) -> String {
    // Regex to extract just the d.d.d version from the full version string
    let captures = PYTHON_VERSION.captures(value).unwrap();
    let version = captures.get(1).unwrap().as_str().to_string();
    if version.ends_with('.') {
        version[..version.len() - 1].to_string()
    } else {
        version
    }
}
