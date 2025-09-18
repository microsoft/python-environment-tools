// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Tests for venv environment detection, including UV-created environments

use std::fs;
use tempfile::TempDir;

use pet_core::pyvenv_cfg::PyVenvCfg;
use pet_core::{env::PythonEnv, python_environment::PythonEnvironmentKind};
use pet_venv::{is_venv, is_venv_dir, is_venv_uv, is_venv_uv_dir, Venv};
use pet_core::Locator;

/// Test that we can detect regular venv environments
#[test]
fn test_detect_regular_venv() {
    let temp_dir = TempDir::new().unwrap();
    let venv_dir = temp_dir.path().join("test_venv");
    fs::create_dir_all(&venv_dir).unwrap();
    
    // Create a regular pyvenv.cfg (without UV)
    let pyvenv_cfg_content = r#"home = /usr/bin
implementation = CPython
version_info = 3.12.11
include-system-site-packages = false
prompt = test_venv
"#;
    let pyvenv_cfg_path = venv_dir.join("pyvenv.cfg");
    fs::write(&pyvenv_cfg_path, pyvenv_cfg_content).unwrap();
    
    // Create a dummy python executable
    let scripts_dir = if cfg!(windows) { "Scripts" } else { "bin" };
    let bin_dir = venv_dir.join(scripts_dir);
    fs::create_dir_all(&bin_dir).unwrap();
    let python_exe = bin_dir.join(if cfg!(windows) { "python.exe" } else { "python" });
    fs::write(&python_exe, "dummy").unwrap();
    
    // Test PyVenvCfg detection
    let cfg = PyVenvCfg::find(&venv_dir).unwrap();
    assert!(!cfg.is_uv());
    assert_eq!(cfg.version, "3.12.11");
    assert_eq!(cfg.prompt, Some("test_venv".to_string()));
    
    // Test directory detection
    assert!(is_venv_dir(&venv_dir));
    assert!(!is_venv_uv_dir(&venv_dir));
    
    // Test with PythonEnv
    let python_env = PythonEnv::new(python_exe.clone(), None, None);
    assert!(is_venv(&python_env));
    assert!(!is_venv_uv(&python_env));
    
    // Test locator
    let locator = Venv::new();
    let result = locator.try_from(&python_env).unwrap();
    assert_eq!(result.kind, Some(PythonEnvironmentKind::Venv));
    assert_eq!(result.name, Some("test_venv".to_string()));
}

/// Test that we can detect UV venv environments
#[test]
fn test_detect_uv_venv() {
    let temp_dir = TempDir::new().unwrap();
    let venv_dir = temp_dir.path().join("test_uv_venv");
    fs::create_dir_all(&venv_dir).unwrap();
    
    // Create a UV pyvenv.cfg (with UV entry)
    let pyvenv_cfg_content = r#"home = /usr/bin
implementation = CPython
uv = 0.8.14
version_info = 3.12.11
include-system-site-packages = false
prompt = test_uv_venv
"#;
    let pyvenv_cfg_path = venv_dir.join("pyvenv.cfg");
    fs::write(&pyvenv_cfg_path, pyvenv_cfg_content).unwrap();
    
    // Create a dummy python executable
    let scripts_dir = if cfg!(windows) { "Scripts" } else { "bin" };
    let bin_dir = venv_dir.join(scripts_dir);
    fs::create_dir_all(&bin_dir).unwrap();
    let python_exe = bin_dir.join(if cfg!(windows) { "python.exe" } else { "python" });
    fs::write(&python_exe, "dummy").unwrap();
    
    // Test PyVenvCfg detection
    let cfg = PyVenvCfg::find(&venv_dir).unwrap();
    assert!(cfg.is_uv());
    assert_eq!(cfg.uv_version, Some("0.8.14".to_string()));
    assert_eq!(cfg.version, "3.12.11");
    assert_eq!(cfg.prompt, Some("test_uv_venv".to_string()));
    
    // Test directory detection
    assert!(is_venv_dir(&venv_dir));
    assert!(is_venv_uv_dir(&venv_dir));
    
    // Test with PythonEnv
    let python_env = PythonEnv::new(python_exe.clone(), None, None);
    assert!(is_venv(&python_env));
    assert!(is_venv_uv(&python_env));
    
    // Test locator
    let locator = Venv::new();
    let result = locator.try_from(&python_env).unwrap();
    assert_eq!(result.kind, Some(PythonEnvironmentKind::VenvUv));
    assert_eq!(result.name, Some("test_uv_venv".to_string()));
}

/// Test that UV version parsing works with different UV version formats
#[test]
fn test_uv_version_parsing() {
    let temp_dir = TempDir::new().unwrap();
    let venv_dir = temp_dir.path().join("test_uv_version");
    fs::create_dir_all(&venv_dir).unwrap();
    
    // Test different UV version formats
    let test_cases = vec![
        ("uv = 0.8.14", Some("0.8.14".to_string())),
        ("uv=0.8.14", Some("0.8.14".to_string())),
        ("uv = 1.0.0-beta", Some("1.0.0-beta".to_string())),
        ("uv= 2.1.3 ", Some("2.1.3".to_string())),
    ];
    
    for (uv_line, expected) in test_cases {
        let pyvenv_cfg_content = format!(
            r#"home = /usr/bin
implementation = CPython
{}
version_info = 3.12.11
include-system-site-packages = false
prompt = test_uv
"#,
            uv_line
        );
        let pyvenv_cfg_path = venv_dir.join("pyvenv.cfg");
        fs::write(&pyvenv_cfg_path, pyvenv_cfg_content).unwrap();
        
        let cfg = PyVenvCfg::find(&venv_dir).unwrap();
        assert_eq!(cfg.uv_version, expected, "Failed for UV line: {}", uv_line);
        assert!(cfg.is_uv());
    }
}

/// Test locator supported categories
#[test]
fn test_locator_supported_categories() {
    let locator = Venv::new();
    let categories = locator.supported_categories();
    
    assert!(categories.contains(&PythonEnvironmentKind::Venv));
    assert!(categories.contains(&PythonEnvironmentKind::VenvUv));
    assert_eq!(categories.len(), 2);
}