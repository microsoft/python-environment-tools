// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod common;
use pet_utils::sys_prefix::SysPrefix;
use std::path::PathBuf;

use common::resolve_test_path;

#[cfg(unix)]
#[test]
fn version_from_sys_prefix() {
    let path: PathBuf = resolve_test_path(&["unix", "pyvenv_cfg", ".venv"]).into();
    let version = SysPrefix::get_version(&path).unwrap();
    assert_eq!(version, "3.12.1");

    let path: PathBuf = resolve_test_path(&["unix", "pyvenv_cfg", ".venv", "bin"]).into();
    let version = SysPrefix::get_version(&path).unwrap();
    assert_eq!(version, "3.12.1");
}

#[cfg(unix)]
#[test]
fn version_from_sys_prefix_using_version_info_format() {
    let path: PathBuf = resolve_test_path(&["unix", "pyvenv_cfg", "hatch_env"]).into();
    let version = SysPrefix::get_version(&path).unwrap();
    assert_eq!(version, "3.9.6.final.0");

    let path: PathBuf = resolve_test_path(&["unix", "pyvenv_cfg", "hatch_env", "bin"]).into();
    let version = SysPrefix::get_version(&path).unwrap();
    assert_eq!(version, "3.9.6.final.0");
}

#[cfg(unix)]
#[test]
fn no_version_without_pyvenv_cfg_and_without_headers() {
    let path: PathBuf =
        resolve_test_path(&["unix", "pyvenv_cfg", "python3.9.9_without_headers"]).into();
    let version = SysPrefix::get_version(&path);
    assert!(version.is_none());

    let path: PathBuf =
        resolve_test_path(&["unix", "pyvenv_cfg", "python3.9.9_without_headers", "bin"]).into();
    let version = SysPrefix::get_version(&path);
    assert!(version.is_none());

    let path: PathBuf = resolve_test_path(&[
        "unix",
        "pyvenv_cfg",
        "python3.9.9_without_headers",
        "bin",
        "python",
    ])
    .into();
    let version = SysPrefix::get_version(&path);
    assert!(version.is_none());
}

#[cfg(unix)]
#[test]
fn no_version_for_invalid_paths() {
    let path: PathBuf = resolve_test_path(&["unix_1234"]).into();
    let version = SysPrefix::get_version(&path);
    assert!(version.is_none());
}

#[cfg(unix)]
#[test]
fn version_from_header_files() {
    let path: PathBuf = resolve_test_path(&["unix", "headers", "python3.9.9"]).into();
    let version = SysPrefix::get_version(&path).unwrap();
    assert_eq!(version, "3.9.9");

    let path: PathBuf = resolve_test_path(&["unix", "headers", "python3.9.9", "bin"]).into();
    let version = SysPrefix::get_version(&path).unwrap();
    assert_eq!(version, "3.9.9");

    let path: PathBuf = resolve_test_path(&["unix", "headers", "python3.10-dev", "bin"]).into();
    let version = SysPrefix::get_version(&path).unwrap();
    assert_eq!(version, "3.10.14+");

    let path: PathBuf = resolve_test_path(&["unix", "headers", "python3.13", "bin"]).into();
    let version = SysPrefix::get_version(&path).unwrap();
    assert_eq!(version, "3.13.0a5");
}
