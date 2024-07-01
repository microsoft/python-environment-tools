// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod common;

#[cfg(unix)]
#[cfg_attr(feature = "ci-jupyter-container", test)]
#[allow(dead_code)]
/// Tests again the container used in https://github.com/github/codespaces-jupyter
fn verify_python_in_jupyter_contaner() {
    use pet::{find::find_and_report_envs, locators::create_locators};
    use pet_conda::Conda;
    use pet_core::{
        arch::Architecture,
        manager::{EnvManager, EnvManagerType},
        os_environment::EnvironmentApi,
        python_environment::{PythonEnvironment, PythonEnvironmentCategory},
    };
    use pet_reporter::test;
    use std::{path::PathBuf, sync::Arc};

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

    let conda = PythonEnvironment {
        category: PythonEnvironmentCategory::Conda,
        name: Some("base".to_string()),
        executable: Some(PathBuf::from("/opt/conda/bin/python")),
        prefix: Some(PathBuf::from("/opt/conda")),
        version: Some("3.12.3".to_string()),
        arch: Some(Architecture::X64),
        symlinks: Some(vec![
            PathBuf::from("/opt/conda/bin/python"),
            PathBuf::from("/opt/conda/bin/python3"),
            PathBuf::from("/opt/conda/bin/python3.1"),
            PathBuf::from("/opt/conda/bin/python3.12"),
        ]),
        manager: Some(EnvManager {
            tool: EnvManagerType::Conda,
            executable: PathBuf::from("/opt/conda/bin/conda"),
            version: Some("24.5.0".to_string()),
        }),
        ..Default::default()
    };
    let codespace_python = PythonEnvironment {
        category: PythonEnvironmentCategory::GlobalPaths,
        executable: Some(PathBuf::from("/home/codespace/.python/current/bin/python")),
        prefix: Some(PathBuf::from("/usr/local/python/3.10.13")),
        version: Some("3.10.13.final.0".to_string()),
        arch: Some(Architecture::X64),
        symlinks: Some(vec![
            PathBuf::from("/home/codespace/.python/current/bin/python"),
            PathBuf::from("/home/codespace/.python/current/bin/python3"),
            PathBuf::from("/home/codespace/.python/current/bin/python3.10"),
        ]),
        manager: None,
        ..Default::default()
    };
    let current_python = PythonEnvironment {
        category: PythonEnvironmentCategory::GlobalPaths,
        executable: Some(PathBuf::from("/usr/local/python/current/bin/python")),
        prefix: Some(PathBuf::from("/usr/local/python/3.10.13")),
        version: Some("3.10.13.final.0".to_string()),
        arch: Some(Architecture::X64),
        symlinks: Some(vec![
            PathBuf::from("/usr/local/python/current/bin/python"),
            PathBuf::from("/usr/local/python/current/bin/python3"),
            PathBuf::from("/usr/local/python/current/bin/python3.10"),
        ]),
        manager: None,
        ..Default::default()
    };
    let usr_bin_python = PythonEnvironment {
        category: PythonEnvironmentCategory::LinuxGlobal,
        executable: Some(PathBuf::from("/usr/bin/python3")),
        prefix: Some(PathBuf::from("/usr")),
        version: Some("3.8.10.final.0".to_string()),
        arch: Some(Architecture::X64),
        symlinks: Some(vec![
            PathBuf::from("/usr/bin/python3"),
            PathBuf::from("/usr/bin/python3.8"),
        ]),
        manager: None,
        ..Default::default()
    };
    let bin_python = PythonEnvironment {
        category: PythonEnvironmentCategory::LinuxGlobal,
        executable: Some(PathBuf::from("/bin/python3")),
        prefix: Some(PathBuf::from("/usr")),
        version: Some("3.8.10.final.0".to_string()),
        arch: Some(Architecture::X64),
        symlinks: Some(vec![
            PathBuf::from("/bin/python3"),
            PathBuf::from("/bin/python3.8"),
        ]),
        manager: None,
        ..Default::default()
    };

    for env in [
        conda,
        codespace_python,
        current_python,
        usr_bin_python,
        bin_python,
    ]
    .iter()
    {
        let python_env = environments
            .iter()
            .find(|e| e.executable == env.executable)
            .expect(format!("Expected to find python environment {:?}", env.executable).as_str());
        assert_eq!(python_env.executable, env.executable);
        assert_eq!(python_env.category, env.category);
        assert_eq!(python_env.symlinks, env.symlinks);
        assert_eq!(python_env.manager, env.manager);
        assert_eq!(python_env.name, env.name);
        assert_eq!(python_env.version, env.version);
        assert_eq!(python_env.arch, env.arch);

        // known issue https://github.com/microsoft/python-environment-tools/issues/64
        if env.executable == Some(PathBuf::from("/home/codespace/.python/current/bin/python")) {
            assert!(
                python_env.prefix == Some(PathBuf::from("/home/codespace/.python/current"))
                    || python_env.prefix == Some(PathBuf::from("/usr/local/python/3.10.13")),
                "Expected {:?} to be {:?} or {:?}",
                python_env.prefix,
                "/home/codespace/.python/current",
                "/usr/local/python/3.10.13"
            );
        }
    }
}
