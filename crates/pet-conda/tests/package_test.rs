// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod common;
use pet_conda::package::{self, CondaPackageInfo};
use std::path::PathBuf;

use common::resolve_test_path;

#[cfg(unix)]
#[test]
fn empty_result_for_bogus_paths() {
    let path: PathBuf = resolve_test_path(&["unix", "bogus_path"]).into();
    let pkg = CondaPackageInfo::from(&path, &package::Package::Conda);

    assert!(pkg.is_none());
}

#[cfg(unix)]
#[test]
fn get_conda_package_info() {
    let path: PathBuf = resolve_test_path(&["unix", "anaconda3-2023.03"]).into();
    let pkg = CondaPackageInfo::from(&path, &package::Package::Conda).unwrap();

    assert_eq!(pkg.package, package::Package::Conda);
    assert_eq!(pkg.version, "23.1.0".to_string());
    assert_eq!(
        pkg.path,
        resolve_test_path(&[
            "unix",
            "anaconda3-2023.03",
            "conda-meta",
            "conda-23.1.0-py310hca03da5_0.json"
        ])
    );
}

#[cfg(unix)]
#[test]
fn get_python_package_info() {
    let path: PathBuf = resolve_test_path(&["unix", "anaconda3-2023.03"]).into();
    let pkg = CondaPackageInfo::from(&path, &package::Package::Python).unwrap();

    assert_eq!(pkg.package, package::Package::Python);
    assert_eq!(pkg.version, "3.10.9".to_string());
    assert_eq!(
        pkg.path,
        resolve_test_path(&[
            "unix",
            "anaconda3-2023.03",
            "conda-meta",
            "python-3.10.9-hc0d8a6c_1.json"
        ])
    );
}

#[cfg(unix)]
#[test]
fn get_conda_package_info_without_history() {
    let path: PathBuf = resolve_test_path(&["unix", "anaconda3-2023.03-without-history"]).into();
    let pkg = CondaPackageInfo::from(&path, &package::Package::Conda).unwrap();

    assert_eq!(pkg.package, package::Package::Conda);
    assert_eq!(pkg.version, "23.1.0".to_string());
    assert_eq!(
        pkg.path,
        resolve_test_path(&[
            "unix",
            "anaconda3-2023.03-without-history",
            "conda-meta",
            "conda-23.1.0-py310hca03da5_0.json"
        ])
    );
}

#[cfg(unix)]
#[test]
fn get_python_package_info_without_history() {
    let path: PathBuf = resolve_test_path(&["unix", "anaconda3-2023.03-without-history"]).into();
    let pkg = CondaPackageInfo::from(&path, &package::Package::Python).unwrap();

    assert_eq!(pkg.package, package::Package::Python);
    assert_eq!(pkg.version, "3.10.9".to_string());
    assert_eq!(
        pkg.path,
        resolve_test_path(&[
            "unix",
            "anaconda3-2023.03-without-history",
            "conda-meta",
            "python-3.10.9-hc0d8a6c_1.json"
        ])
    );
}
