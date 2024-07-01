// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::path::PathBuf;

use common::{does_version_match, resolve_test_path};
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
#[cfg_attr(
    any(
        feature = "ci",
        feature = "ci-jupyter-container",
        feature = "ci-homebrew-container"
    ),
    test
)]
#[allow(dead_code)]
// We should detect the conda install along with the base env
fn verify_validity_of_discovered_envs() {
    use pet::{find::find_and_report_envs, locators::create_locators};
    use pet_conda::Conda;
    use pet_core::{os_environment::EnvironmentApi, Configuration};
    use pet_reporter::test;
    use std::{env, sync::Arc, thread};

    let reporter = test::create_reporter();
    let environment = EnvironmentApi::new();
    let conda_locator = Arc::new(Conda::from(&environment));
    let mut config = Configuration::default();
    if let Ok(cwd) = env::current_dir() {
        config.search_paths = Some(vec![cwd]);
    }
    let locators = create_locators(conda_locator.clone());
    for locator in locators.iter() {
        locator.configure(&config);
    }

    find_and_report_envs(&reporter, Default::default(), &locators, conda_locator);
    let result = reporter.get_result();

    let environments = result.environments;
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

#[cfg(unix)]
#[cfg(target_os = "linux")]
#[cfg_attr(feature = "ci", test)]
#[allow(dead_code)]
// On linux we create a virtualenvwrapper environment named `venv_wrapper_env1`
fn check_if_virtualenvwrapper_exists() {
    use pet::{find::find_and_report_envs, locators::create_locators};
    use pet_conda::Conda;
    use pet_core::os_environment::EnvironmentApi;
    use pet_reporter::test;
    use std::sync::Arc;

    let reporter = test::create_reporter();
    let environment = EnvironmentApi::new();
    let conda_locator = Arc::new(Conda::from(&environment));

    find_and_report_envs(
        &reporter,
        Default::default(),
        &create_locators(conda_locator.clone()),
        conda_locator,
    );

    let result = reporter.get_result();
    let environments = result.environments;

    assert!(
        environments.iter().any(
            |env| env.category == PythonEnvironmentCategory::VirtualEnvWrapper
                && env.executable.is_some()
                && env.prefix.is_some()
                && env
                    .executable
                    .clone()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default()
                    .contains("venv_wrapper_env1")
        ),
        "Virtualenvwrapper environment not found, found: {:?}",
        environments
    );
}

#[cfg(unix)]
#[cfg(target_os = "linux")]
#[cfg_attr(feature = "ci", test)]
#[allow(dead_code)]
// On linux we create a virtualenvwrapper environment named `venv_wrapper_env1`
fn check_if_pyenv_virtualenv_exists() {
    use pet::{find::find_and_report_envs, locators::create_locators};
    use pet_conda::Conda;
    use pet_core::os_environment::EnvironmentApi;
    use pet_reporter::test;
    use std::sync::Arc;

    let reporter = test::create_reporter();
    let environment = EnvironmentApi::new();
    let conda_locator = Arc::new(Conda::from(&environment));

    find_and_report_envs(
        &reporter,
        Default::default(),
        &create_locators(conda_locator.clone()),
        conda_locator,
    );

    let result = reporter.get_result();
    let environments = result.environments;

    assert!(
        environments.iter().any(
            |env| env.category == PythonEnvironmentCategory::PyenvVirtualEnv
                && env.executable.is_some()
                && env.prefix.is_some()
                && env.manager.is_some()
                && env
                    .executable
                    .clone()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default()
                    .contains("pyenv-virtualenv-env1")
        ),
        "pyenv-virtualenv environment not found, found: {:?}",
        environments
    );
}

fn verify_validity_of_interpreter_info(environment: PythonEnvironment) {
    let run_command = get_python_run_command(&environment);
    let interpreter_info = get_python_interpreter_info(&run_command);

    // Home brew has too many syminks, unfortunately its not easy to test in CI.
    if environment.category != PythonEnvironmentCategory::Homebrew {
        let expected_executable = environment.executable.clone().unwrap();

        // Ensure the executable is in one of the identified symlinks
        assert!(
            environment
                .symlinks
                .clone()
                .unwrap_or_default()
                .contains(&PathBuf::from(expected_executable)),
            "Executable mismatch for {:?}",
            environment.clone()
        );
    }
    // If this is a conda env, then the manager, prefix and a few things must exist.
    if environment.category == PythonEnvironmentCategory::Conda {
        assert!(environment.manager.is_some());
        assert!(environment.prefix.is_some());
        if environment.executable.is_some() {
            // Version must exist in this case.
            assert!(environment.version.is_some());
        }
    }
    if let Some(prefix) = environment.clone().prefix {
        if interpreter_info.clone().executable == "/usr/local/python/current/bin/python"
            && (prefix.to_str().unwrap() == "/usr/local/python/current"
                && interpreter_info.clone().sys_prefix == "/usr/local/python/3.10.13")
            || (prefix.to_str().unwrap() == "/usr/local/python/3.10.13"
                && interpreter_info.clone().sys_prefix == "/usr/local/python/current")
        {
            // known issue https://github.com/microsoft/python-environment-tools/issues/64
        } else if interpreter_info.clone().executable
            == "/home/codespace/.python/current/bin/python"
            && (prefix.to_str().unwrap() == "/home/codespace/.python/current"
                && interpreter_info.clone().sys_prefix == "/usr/local/python/3.10.13")
            || (prefix.to_str().unwrap() == "/usr/local/python/3.10.13"
                && interpreter_info.clone().sys_prefix == "/home/codespace/.python/current")
        {
            // known issue https://github.com/microsoft/python-environment-tools/issues/64
        } else {
            assert_eq!(
                prefix.to_str().unwrap(),
                interpreter_info.clone().sys_prefix,
                "Prefix mismatch for {:?}",
                environment.clone()
            );
        }
    }
    if let Some(arch) = environment.clone().arch {
        let expected_arch = if interpreter_info.clone().is64_bit {
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
        let expected_version = &interpreter_info.clone().sys_version;
        assert!(
            does_version_match(&version, expected_version),
            "Version mismatch for (expected {:?} to start with {:?}) for {:?}",
            expected_version,
            version,
            environment.clone()
        );
    }
}

#[cfg(unix)]
#[cfg(target_os = "linux")]
#[cfg_attr(feature = "ci", test)]
#[allow(dead_code)]
// On linux we /bin/python, /usr/bin/python and /usr/local/python are all separate environments.
fn verify_bin_usr_bin_user_local_are_separate_python_envs() {
    use pet::{find::find_and_report_envs, locators::create_locators};
    use pet_conda::Conda;
    use pet_core::os_environment::EnvironmentApi;
    use pet_reporter::test;
    use std::sync::Arc;

    let reporter = test::create_reporter();
    let environment = EnvironmentApi::new();
    let conda_locator = Arc::new(Conda::from(&environment));

    find_and_report_envs(
        &reporter,
        Default::default(),
        &create_locators(conda_locator.clone()),
        conda_locator,
    );

    let result = reporter.get_result();
    let environments = result.environments;

    // Python env /bin/python cannot have symlinks in /usr/bin or /usr/local
    // Python env /usr/bin/python cannot have symlinks /bin or /usr/local
    // Python env /usr/local/bin/python cannot have symlinks in /bin or /usr/bin
    let bins = ["/bin", "/usr/bin", "/usr/local/bin"];
    for bin in bins.iter() {
        if let Some(bin_python) = environments.iter().find(|e| {
            e.executable.clone().is_some()
                && e.executable
                    .clone()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .starts_with(bin)
        }) {
            // If the exe is in /bin, then we can never have any symlinks to other folders such as /usr/bin or /usr/local
            let other_bins = bins
                .iter()
                .filter(|b| *b != bin)
                .map(|b| PathBuf::from(*b))
                .collect::<Vec<PathBuf>>();
            if let Some(symlinks) = &bin_python.symlinks {
                for symlink in symlinks.iter() {
                    let parent_of_symlink = symlink.parent().unwrap().to_path_buf();
                    if other_bins.contains(&parent_of_symlink) {
                        panic!(
                            "Python environment {:?} cannot have a symlinks in {:?}",
                            bin_python, other_bins
                        );
                    }
                }
            }
        }
    }
}

#[allow(dead_code)]
fn get_conda_exe() -> &'static str {
    // On CI we expect conda to be in the current path.
    "conda"
}

#[derive(Deserialize, Clone)]
struct InterpreterInfo {
    sys_prefix: String,
    #[allow(dead_code)]
    executable: String,
    sys_version: String,
    is64_bit: bool,
    // version_info: (u16, u16, u16, String, u16),
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
