// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod common;
use pet_conda::utils;
use std::path::PathBuf;

use common::resolve_test_path;

#[test]
fn is_conda_install() {
    let path: PathBuf = resolve_test_path(&["unix", "anaconda3-2023.03"]).into();
    assert!(utils::is_conda_install(&path));

    let path: PathBuf = resolve_test_path(&["unix", "anaconda3-2023.03-without-history"]).into();
    assert!(utils::is_conda_install(&path));
}

#[test]
fn is_not_conda_install() {
    let path: PathBuf = resolve_test_path(&["unix", "some bogus directory"]).into();
    assert_eq!(utils::is_conda_install(&path), false);

    // Conda env is not an install location.
    let path: PathBuf =
        resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "env_python_3"]).into();
    assert_eq!(utils::is_conda_install(&path), false);
}

#[test]
fn is_conda_env() {
    let path: PathBuf = resolve_test_path(&["unix", "anaconda3-2023.03"]).into();
    assert!(utils::is_conda_env(&path));

    let path: PathBuf = resolve_test_path(&["unix", "anaconda3-2023.03-without-history"]).into();
    assert!(utils::is_conda_env(&path));

    let path: PathBuf =
        resolve_test_path(&["unix", "anaconda3-2023.03", "envs", "env_python_3"]).into();
    assert!(utils::is_conda_env(&path));
}

#[test]
fn is_not_conda_env() {
    let path: PathBuf = resolve_test_path(&["unix", "some bogus directory"]).into();
    assert_eq!(utils::is_conda_env(&path), false);

    let path: PathBuf = resolve_test_path(&["unix", "anaconda3-2023.03"]).into();
    assert!(utils::is_conda_env(&path));

    let path: PathBuf = resolve_test_path(&["unix", "anaconda3-2023.03-without-history"]).into();
    assert!(utils::is_conda_env(&path));
}
