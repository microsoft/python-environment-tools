// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod common;

#[cfg(unix)]
#[test]
fn find_conda_env_without_manager() {
    use common::{create_test_environment, resolve_test_path};
    use pet_conda::Conda;
    use pet_core::{self, arch::Architecture, python_environment::PythonEnvironmentKind, Locator};
    use pet_python_utils::env::PythonEnv;
    use std::collections::HashMap;

    let environment = create_test_environment(HashMap::new(), None, vec![], None);
    let locator = Conda::from(&environment);
    let path = resolve_test_path(&["unix", "conda_env_without_manager", "env_python_3"]);

    let env = locator
        .try_from(&PythonEnv::new(
            path.join("bin").join("python").into(),
            Some(path.clone().into()),
            None,
        ))
        .unwrap();

    assert_eq!(env.prefix, path.clone().into());
    assert_eq!(env.arch, Architecture::X64.into());
    assert_eq!(env.kind, Some(PythonEnvironmentKind::Conda));
    assert_eq!(env.executable, path.join("bin").join("python").into());
    assert_eq!(env.version, "3.12.2".to_string().into());
    assert_eq!(env.manager, None);
    assert_eq!(env.name, "env_python_3".to_string().into());
}

#[cfg(unix)]
#[test]
fn find_conda_env_without_manager_but_detect_manager_from_history() {
    use common::{create_test_environment, resolve_test_path};
    use pet_conda::Conda;
    use pet_core::{self, arch::Architecture, python_environment::PythonEnvironmentKind, Locator};
    use pet_python_utils::env::PythonEnv;
    use std::{
        collections::HashMap,
        fs::{self},
    };

    let environment = create_test_environment(HashMap::new(), None, vec![], None);
    let locator = Conda::from(&environment);
    let path = resolve_test_path(&[
        "unix",
        "conda_env_without_manager_but_found_in_history",
        "env_python_3",
    ]);
    let conda_dir = resolve_test_path(&[
        "unix",
        "conda_env_without_manager_but_found_in_history",
        "some_other_location",
        "conda_install",
    ]);
    let history_file = path.join("conda-meta").join("history");
    let history_file_template = path.join("conda-meta").join("history_template");
    let history_contents = fs::read_to_string(&history_file_template)
        .unwrap()
        .replace("<CONDA_INSTALL>", conda_dir.to_str().unwrap_or_default());
    fs::write(history_file, history_contents).unwrap();

    let env = locator
        .try_from(&PythonEnv::new(
            path.join("bin").join("python").into(),
            Some(path.clone().into()),
            None,
        ))
        .unwrap();

    assert_eq!(env.prefix, path.clone().into());
    assert_eq!(env.arch, Architecture::X64.into());
    assert_eq!(env.kind, Some(PythonEnvironmentKind::Conda));
    assert_eq!(env.executable, path.join("bin").join("python").into());
    assert_eq!(env.version, "3.12.2".to_string().into());
    assert_eq!(
        env.manager.clone().unwrap().executable,
        conda_dir.join("bin").join("conda")
    );
    assert_eq!(
        env.manager.clone().unwrap().version,
        "23.1.0".to_string().into()
    );
    assert_eq!(env.name, None);
}
