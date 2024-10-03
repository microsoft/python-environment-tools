// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::sync::Once;

mod common;

static INIT: Once = Once::new();

/// Setup function that is only run once, even if called multiple times.
fn setup() {
    INIT.call_once(|| {
        env_logger::builder()
            .filter(None, log::LevelFilter::Trace)
            .init();
    });
}

#[cfg(unix)]
#[cfg_attr(feature = "ci-homebrew-container", test)]
#[allow(dead_code)]
fn verify_python_in_homebrew_contaner() {
    use pet::{find::find_and_report_envs, locators::create_locators};
    use pet_conda::Conda;
    use pet_core::{
        os_environment::EnvironmentApi,
        python_environment::{PythonEnvironment, PythonEnvironmentKind},
    };
    use pet_poetry::Poetry;
    use pet_reporter::{cache::CacheReporter, collect};
    use std::{path::PathBuf, sync::Arc};

    setup();

    let reporter = Arc::new(collect::create_reporter());
    let environment = EnvironmentApi::new();
    let conda_locator = Arc::new(Conda::from(&environment));
    let poetry_locator = Arc::new(Poetry::from(&environment));

    find_and_report_envs(
        &CacheReporter::new(reporter.clone()),
        Default::default(),
        &create_locators(conda_locator.clone(), poetry_locator.clone(), &environment),
        &environment,
        None,
    );

    let environments = reporter.environments.lock().unwrap().clone();

    let python3_12 = PythonEnvironment {
        kind: Some(PythonEnvironmentKind::Homebrew),
        executable: Some(PathBuf::from("/home/linuxbrew/.linuxbrew/bin/python3")),
        version: Some("3.12.7".to_string()), // This can change on CI, so we don't check it
        symlinks: Some(vec![
            PathBuf::from("/home/linuxbrew/.linuxbrew/bin/python3"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/bin/python3.12"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/opt/python3/bin/python3"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/opt/python3/bin/python3.12"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/opt/python@3/bin/python3"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/opt/python@3/bin/python3.12"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/opt/python@3.12/bin/python3"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/opt/python@3.12/bin/python3.12"),
            // On CI the Python version can change with minor updates, so we don't check the full version.
            // PathBuf::from("/home/linuxbrew/.linuxbrew/Cellar/python@3.12/3.12.6/bin/python3.12"),
        ]),
        ..Default::default()
    };
    let python3_11 = PythonEnvironment {
        kind: Some(PythonEnvironmentKind::Homebrew),
        executable: Some(PathBuf::from("/home/linuxbrew/.linuxbrew/bin/python3.11")),
        version: Some("3.11.10".to_string()), // This can change on CI, so we don't check it
        symlinks: Some(vec![
            PathBuf::from("/home/linuxbrew/.linuxbrew/bin/python3.11"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/opt/python@3.11/bin/python3.11"),
            // On CI the Python version can change with minor updates, so we don't check the full version.
            // PathBuf::from("/home/linuxbrew/.linuxbrew/Cellar/python@3.11/3.11.10/bin/python3.11"),
        ]),
        ..Default::default()
    };

    assert_eq!(environments.len(), 2);

    for env in [python3_11, python3_12].iter() {
        let python_env = environments
            .iter()
            .find(|e| e.executable == env.executable)
            .expect(format!("Expected to find python environment {:?}", env.executable).as_str());
        assert_eq!(python_env.executable, env.executable);
        assert_eq!(python_env.kind, env.kind);
        assert_eq!(python_env.manager, env.manager);
        // Compare the first 4 parts (3.12)
        assert_eq!(
            python_env.version.clone().unwrap_or_default()[..4],
            env.version.clone().unwrap_or_default()[..4]
        );

        // We know the symlinks contain the full version, hence exclude the paths that contain the full version.
        let python_env_symlinks = python_env
            .symlinks
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| {
                !p.to_string_lossy()
                    .contains(&env.version.clone().unwrap_or_default())
                    && !p
                        .to_string_lossy()
                        .contains(&python_env.version.clone().unwrap_or_default())
            })
            .collect::<Vec<PathBuf>>();
        assert_eq!(
            python_env_symlinks,
            env.symlinks.clone().unwrap_or_default()
        );
    }
}
