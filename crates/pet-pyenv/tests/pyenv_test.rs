// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod common;

#[test]
#[cfg(unix)]
fn does_not_find_any_pyenv_envs() {
    use common::create_test_environment;
    use pet_conda::Conda;
    use pet_core::{self, Locator};
    use pet_pyenv;
    use pet_pyenv::PyEnv;
    use pet_reporter::test::create_reporter;
    use std::{collections::HashMap, path::PathBuf, sync::Arc};

    let environment = create_test_environment(
        HashMap::new(),
        Some(PathBuf::from("SOME_BOGUS_HOME_DIR")),
        vec![],
        None,
    );

    let conda = Arc::new(Conda::from(&environment));
    let locator = PyEnv::from(&environment, conda);
    let reporter = create_reporter();
    locator.find(&reporter);
    let result = reporter.get_result();

    assert_eq!(result.managers.is_empty(), true);
    assert_eq!(result.environments.is_empty(), true);
}

#[test]
#[cfg(unix)]
fn does_not_find_any_pyenv_envs_even_with_pyenv_installed() {
    use crate::common::create_test_environment;
    use common::resolve_test_path;
    use pet_conda::Conda;
    use pet_core::{
        self,
        manager::{EnvManager, EnvManagerType},
        Locator,
    };
    use pet_pyenv;
    use pet_pyenv::PyEnv;
    use pet_reporter::test::create_reporter;
    use serde_json::json;
    use std::{collections::HashMap, path::PathBuf, sync::Arc};

    let home = resolve_test_path(&["unix", "pyenv_without_envs", "user_home"]);
    let homebrew_bin = resolve_test_path(&[
        "unix",
        "pyenv_without_envs",
        "home",
        "opt",
        "homebrew",
        "bin",
    ]);
    let pyenv_exe = resolve_test_path(&[homebrew_bin.to_str().unwrap(), "pyenv"]);
    let environment = create_test_environment(
        HashMap::new(),
        Some(home.clone()),
        vec![PathBuf::from(homebrew_bin)],
        None,
    );

    let conda = Arc::new(Conda::from(&environment));
    let locator = PyEnv::from(&environment, conda);
    let reporter = create_reporter();
    locator.find(&reporter);
    let result = reporter.get_result();

    let managers = result.clone().managers;
    assert_eq!(managers.len(), 1);

    let expected_manager = EnvManager {
        executable: pyenv_exe.clone(),
        version: None,
        tool: EnvManagerType::Pyenv,
    };
    assert_eq!(json!(expected_manager), json!(result.managers[0]));
}

#[test]
#[cfg(unix)]
fn find_pyenv_envs() {
    use crate::common::create_test_environment;
    use common::resolve_test_path;
    use pet_conda::Conda;
    use pet_core::{
        self,
        arch::Architecture,
        manager::{EnvManager, EnvManagerType},
        python_environment::{PythonEnvironment, PythonEnvironmentKind},
        Locator,
    };
    use pet_pyenv;
    use pet_pyenv::PyEnv;
    use pet_reporter::test::create_reporter;
    use serde_json::json;
    use std::{collections::HashMap, path::PathBuf, sync::Arc};

    let home = resolve_test_path(&["unix", "pyenv", "user_home"]);
    let homebrew_bin = resolve_test_path(&["unix", "pyenv", "home", "opt", "homebrew", "bin"]);
    let pyenv_exe = resolve_test_path(&[homebrew_bin.to_str().unwrap(), "pyenv"]);
    let conda_dir = resolve_test_path(&[
        "unix",
        "pyenv",
        "user_home",
        ".pyenv",
        "versions",
        "anaconda-4.0.0",
    ]);
    let conda_exe = conda_dir.join("bin").join("conda");

    let environment = create_test_environment(
        HashMap::new(),
        Some(home.clone()),
        vec![PathBuf::from(homebrew_bin)],
        None,
    );

    let conda = Arc::new(Conda::from(&environment));
    let locator = PyEnv::from(&environment, conda);
    let reporter = create_reporter();
    locator.find(&reporter);
    let mut result = reporter.get_result();

    assert_eq!(result.managers.len(), 2);

    let expected_pyenv_manager = EnvManager {
        executable: pyenv_exe.clone(),
        version: None,
        tool: EnvManagerType::Pyenv,
    };
    let expected_conda_manager = EnvManager {
        executable: conda_exe.clone(),
        version: Some("23.11.0".to_string()),
        tool: EnvManagerType::Conda,
    };

    let mut expected = vec![
        expected_pyenv_manager.clone(),
        expected_conda_manager.clone(),
    ];
    result.managers.sort();
    expected.sort();
    assert_eq!(expected, result.managers);

    let expected_3_9_9 = PythonEnvironment {
        display_name: None,
        project: None,
        name: None,
        executable: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.9.9/bin/python",
        ])),
        kind: Some(PythonEnvironmentKind::Pyenv),
        version: Some("3.9.9".to_string()),
        prefix: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.9.9",
        ])),
        manager: Some(expected_pyenv_manager.clone()),
        arch: None,
        symlinks: Some(vec![resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.9.9/bin/python",
        ])]),
        ..Default::default()
    };
    let expected_virtual_env = PythonEnvironment {
        display_name: None,
        project: None,
        name: None,
        executable: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/my-virtual-env/bin/python",
        ])),
        kind: Some(PythonEnvironmentKind::PyenvVirtualEnv),
        version: Some("3.10.13".to_string()),
        prefix: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/my-virtual-env",
        ])),
        manager: Some(expected_pyenv_manager.clone()),
        arch: None,
        symlinks: Some(vec![resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/my-virtual-env/bin/python",
        ])]),
        ..Default::default()
    };
    let expected_3_12_1 = PythonEnvironment {
        display_name: None,
        project: None,
        name: None,
        executable: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.12.1/bin/python",
        ])),
        kind: Some(PythonEnvironmentKind::Pyenv),
        version: Some("3.12.1".to_string()),
        prefix: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.12.1",
        ])),
        manager: Some(expected_pyenv_manager.clone()),
        arch: None,
        symlinks: Some(vec![resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.12.1/bin/python",
        ])]),
        ..Default::default()
    };
    let expected_3_13_dev = PythonEnvironment {
        display_name: None,
        project: None,
        name: None,
        executable: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.13-dev/bin/python",
        ])),
        kind: Some(PythonEnvironmentKind::Pyenv),
        version: Some("3.13-dev".to_string()),
        prefix: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.13-dev",
        ])),
        manager: Some(expected_pyenv_manager.clone()),
        arch: None,
        symlinks: Some(vec![resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.13-dev/bin/python",
        ])]),
        ..Default::default()
    };
    let expected_3_12_1a3 = PythonEnvironment {
        display_name: None,
        project: None,
        name: None,
        executable: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.12.1a3/bin/python",
        ])),
        kind: Some(PythonEnvironmentKind::Pyenv),
        version: Some("3.12.1a3".to_string()),
        prefix: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.12.1a3",
        ])),
        manager: Some(expected_pyenv_manager.clone()),
        arch: None,
        symlinks: Some(vec![resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.12.1a3/bin/python",
        ])]),
        ..Default::default()
    };
    let expected_no_gil = PythonEnvironment {
        display_name: None,
        project: None,
        name: None,
        executable: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/nogil-3.9.10-1/bin/python",
        ])),
        kind: Some(PythonEnvironmentKind::Pyenv),
        version: Some("3.9.10".to_string()),
        prefix: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/nogil-3.9.10-1",
        ])),
        manager: Some(expected_pyenv_manager.clone()),
        arch: None,
        symlinks: Some(vec![resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/nogil-3.9.10-1/bin/python",
        ])]),
        ..Default::default()
    };
    let expected_pypy = PythonEnvironment {
        display_name: None,
        project: None,
        name: None,
        executable: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/pypy3.9-7.3.15/bin/python",
        ])),
        kind: Some(PythonEnvironmentKind::Pyenv),
        version: Some("3.9.18".to_string()),
        prefix: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/pypy3.9-7.3.15",
        ])),
        manager: Some(expected_pyenv_manager.clone()),
        arch: None,
        symlinks: Some(vec![resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/pypy3.9-7.3.15/bin/python",
        ])]),
        ..Default::default()
    };

    let expected_conda_root = PythonEnvironment {
        display_name: None,
        project: None,
        name: Some("base".to_string()),
        executable: Some(conda_dir.join("bin").join("python")),
        kind: Some(PythonEnvironmentKind::Conda),
        version: Some("3.11.5".to_string()),
        prefix: Some(conda_dir.clone()),
        manager: Some(expected_conda_manager.clone()),
        arch: Some(Architecture::X64),
        symlinks: Some(vec![conda_dir.join("bin").join("python")]),
        ..Default::default()
    };
    let expected_conda_one = PythonEnvironment {
        display_name: None,
        project: None,
        name: Some("one".to_string()),
        executable: Some(conda_dir.join("envs").join("one").join("python")),
        kind: Some(PythonEnvironmentKind::Conda),
        version: Some("3.11.1".to_string()),
        prefix: Some(conda_dir.join("envs").join("one")),
        manager: Some(expected_conda_manager.clone()),
        arch: None,
        symlinks: Some(vec![conda_dir.join("envs").join("one").join("python")]),
        ..Default::default()
    };
    let expected_conda_two = PythonEnvironment {
        display_name: None,
        project: None,
        name: Some("two".to_string()),
        executable: Some(conda_dir.join("envs").join("two").join("python")),
        kind: Some(PythonEnvironmentKind::Conda),
        version: Some("3.11.1".to_string()),
        prefix: Some(conda_dir.join("envs").join("two")),
        manager: Some(expected_conda_manager.clone()),
        symlinks: Some(vec![conda_dir.join("envs").join("two").join("python")]),
        arch: None,
        ..Default::default()
    };

    let mut expected_envs = vec![
        expected_3_9_9,
        expected_virtual_env,
        expected_3_12_1,
        expected_3_13_dev,
        expected_3_12_1a3,
        expected_conda_root,
        expected_conda_one,
        expected_conda_two,
        expected_no_gil,
        expected_pypy,
    ];
    expected_envs.sort();
    result.environments.sort();
    assert_eq!(json!(expected_envs), json!(result.environments));
}

#[test]
#[cfg(unix)]
fn resolve_pyenv_environment() {
    use crate::common::create_test_environment;
    use common::resolve_test_path;
    use pet_conda::Conda;
    use pet_core::{
        self,
        manager::{EnvManager, EnvManagerType},
        python_environment::{PythonEnvironment, PythonEnvironmentKind},
        Locator,
    };
    use pet_pyenv;
    use pet_pyenv::PyEnv;
    use pet_python_utils::env::PythonEnv;
    use std::{collections::HashMap, sync::Arc};

    let home = resolve_test_path(&["unix", "pyenv", "user_home"]);
    let homebrew_bin = resolve_test_path(&["unix", "pyenv", "home", "opt", "homebrew", "bin"]);
    let pyenv_exe = resolve_test_path(&[homebrew_bin.to_str().unwrap(), "pyenv"]);

    let environment =
        create_test_environment(HashMap::new(), Some(home.clone()), vec![homebrew_bin], None);

    let conda = Arc::new(Conda::from(&environment));
    let locator = PyEnv::from(&environment, conda.clone());
    // let mut result = locator.find().unwrap();

    let expected_manager = EnvManager {
        executable: pyenv_exe.clone(),
        version: None,
        tool: EnvManagerType::Pyenv,
    };

    let executable =
        resolve_test_path(&[home.to_str().unwrap(), ".pyenv/versions/3.9.9/bin/python"]);
    let expected_3_9_9 = PythonEnvironment {
        display_name: None,
        project: None,
        name: None,
        executable: Some(executable.clone()),
        kind: Some(PythonEnvironmentKind::Pyenv),
        version: Some("3.9.9".to_string()),
        prefix: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.9.9",
        ])),
        manager: Some(expected_manager.clone()),
        arch: None,
        symlinks: Some(vec![executable]),
        ..Default::default()
    };
    let expected_virtual_env = PythonEnvironment {
        display_name: None,
        project: None,
        name: None,
        executable: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/my-virtual-env/bin/python",
        ])),
        kind: Some(PythonEnvironmentKind::PyenvVirtualEnv),
        version: Some("3.10.13".to_string()),
        prefix: Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/my-virtual-env",
        ])),
        manager: Some(expected_manager.clone()),
        arch: None,
        symlinks: Some(vec![resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/my-virtual-env/bin/python",
        ])]),
        ..Default::default()
    };

    // Resolve regular Python installs in Pyenv
    let result = locator.try_from(&PythonEnv::new(
        resolve_test_path(&[home.to_str().unwrap(), ".pyenv/versions/3.9.9/bin/python"]),
        Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/3.9.9",
        ])),
        None,
    ));

    assert_eq!(result.unwrap(), expected_3_9_9);

    // Resolve regular virtual-envs in Pyenv
    let result = locator.try_from(&PythonEnv::new(
        resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/my-virtual-env/bin/python",
        ]),
        Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/my-virtual-env",
        ])),
        None,
    ));

    assert_eq!(result.unwrap(), expected_virtual_env);

    // Should not resolve conda envs in pyenv
    let result = locator.try_from(&PythonEnv::new(
        resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/anaconda-4.0.0/bin/python",
        ]),
        Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/anaconda-4.0.0",
        ])),
        None,
    ));

    assert_eq!(result.is_none(), true);

    // Should not resolve conda envs using Conda Locator
    let result = conda.try_from(&PythonEnv::new(
        resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/anaconda-4.0.0/bin/python",
        ]),
        Some(resolve_test_path(&[
            home.to_str().unwrap(),
            ".pyenv/versions/anaconda-4.0.0",
        ])),
        None,
    ));

    assert_eq!(result.is_some(), true);
    assert_eq!(result.unwrap().kind, Some(PythonEnvironmentKind::Conda));
}
