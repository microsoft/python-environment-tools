// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

use pet_python_utils::{env::ResolvedPythonEnv, is_pyenv_shim};

#[test]
fn test_pyenv_shim_detection_by_path() {
    // Test path-based detection
    let shim_path = PathBuf::from("/home/user/.pyenv/shims/python3.10");
    assert!(is_pyenv_shim(&shim_path));
    
    // Test that regular paths are not detected as shims
    let regular_path = PathBuf::from("/usr/bin/python3");
    assert!(!is_pyenv_shim(&regular_path));
}

#[test]
fn test_pyenv_shim_detection_by_content() {
    // Create a temporary file with pyenv shim content
    let mut shim_file = NamedTempFile::new().unwrap();
    let shim_content = r#"#!/usr/bin/env bash
set -e
[ -n "$PYENV_DEBUG" ] && set -x

program="${0##*/}"

export PYENV_ROOT="/home/user/.pyenv"
exec "/home/user/.pyenv/libexec/pyenv" exec "$program" "$@"
"#;
    shim_file.write_all(shim_content.as_bytes()).unwrap();
    
    // Make it executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(shim_file.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(shim_file.path(), perms).unwrap();
    }
    
    // Test that this is detected as a pyenv shim
    assert!(is_pyenv_shim(shim_file.path()));
}

#[test]
fn test_pyenv_shim_resolve_returns_none() {
    // Create a temporary file that looks like a pyenv shim
    let mut shim_file = NamedTempFile::new().unwrap();
    let shim_content = r#"#!/usr/bin/env bash
export PYENV_ROOT="/home/user/.pyenv"
exec "/home/user/.pyenv/libexec/pyenv" exec "$program" "$@"
"#;
    shim_file.write_all(shim_content.as_bytes()).unwrap();
    
    // Make it executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(shim_file.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(shim_file.path(), perms).unwrap();
    }
    
    // Test that we cannot resolve this pyenv shim
    let result = ResolvedPythonEnv::from(shim_file.path());
    
    // The result should be None since pyenv shims should be skipped
    assert!(result.is_none());
}

#[test]
fn test_regular_script_not_detected_as_shim() {
    // Create a regular script that shouldn't be detected as a pyenv shim
    let mut script_file = NamedTempFile::new().unwrap();
    let script_content = r#"#!/usr/bin/env bash
echo "Hello World"
python3 --version
"#;
    script_file.write_all(script_content.as_bytes()).unwrap();
    
    // Test that this is not detected as a pyenv shim
    assert!(!is_pyenv_shim(script_file.path()));
}