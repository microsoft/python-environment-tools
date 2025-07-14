use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

use pet_core::{
    env::PythonEnv, 
    os_environment::Environment, 
    reporter::Reporter,
    python_environment::PythonEnvironment,
    Locator
};
use pet_uv::Uv;

struct TestReporter {
    environments: std::sync::Mutex<Vec<PythonEnvironment>>,
}

impl TestReporter {
    fn new() -> Self {
        Self {
            environments: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn get_environments(&self) -> Vec<PythonEnvironment> {
        self.environments.lock().unwrap().clone()
    }
}

impl Reporter for TestReporter {
    fn report_environment(&self, env: &PythonEnvironment) {
        self.environments.lock().unwrap().push(env.clone());
    }

    fn report_manager(&self, _manager: &pet_core::manager::EnvManager) {
        // Not needed for this test
    }

    fn report_telemetry(&self, _event: &pet_core::telemetry::TelemetryEvent) {
        // Not needed for this test
    }
}

struct TestEnvironment {
    env_vars: std::collections::HashMap<String, String>,
}

impl TestEnvironment {
    fn new() -> Self {
        Self {
            env_vars: std::collections::HashMap::new(),
        }
    }

    fn with_env_var(mut self, key: &str, value: &str) -> Self {
        self.env_vars.insert(key.to_string(), value.to_string());
        self
    }
}

impl Environment for TestEnvironment {
    fn get_user_home(&self) -> Option<PathBuf> {
        self.env_vars.get("HOME").map(|h| PathBuf::from(h))
    }

    fn get_root(&self) -> Option<PathBuf> {
        None
    }

    fn get_env_var(&self, key: String) -> Option<String> {
        self.env_vars.get(&key).cloned()
    }

    fn get_know_global_search_locations(&self) -> Vec<PathBuf> {
        vec![]
    }
}

#[test]
fn test_uv_locator_integration() {
    // Create a temporary directory structure to mimic UV layout
    let temp_dir = TempDir::new().unwrap();
    let uv_data_dir = temp_dir.path().join("uv");
    let python_dir = uv_data_dir.join("python");
    let python_install_dir = python_dir.join("cpython-3.11.5-linux-x86_64-gnu");
    let bin_dir = python_install_dir.join("bin");
    
    // Create the directory structure
    fs::create_dir_all(&bin_dir).unwrap();
    
    // Create a fake Python executable
    let python_exe = bin_dir.join("python");
    fs::write(&python_exe, "#!/usr/bin/env python3\nprint('Hello')\n").unwrap();
    
    // Make it executable (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&python_exe, fs::Permissions::from_mode(0o755)).unwrap();
    }
    
    // Create test environment with UV_DATA_DIR pointing to our temp directory
    let env = TestEnvironment::new()
        .with_env_var("UV_DATA_DIR", uv_data_dir.to_str().unwrap());
    
    // Create UV locator
    let uv_locator = Uv::from(&env);
    
    // Test that it can identify UV python environments
    let python_env = PythonEnv {
        executable: python_exe.clone(),
        prefix: Some(python_install_dir.clone()),
        version: None,
        symlinks: None,
    };
    
    let detected_env = uv_locator.try_from(&python_env);
    assert!(detected_env.is_some());
    
    let detected_env = detected_env.unwrap();
    assert_eq!(detected_env.kind, Some(pet_core::python_environment::PythonEnvironmentKind::Uv));
    assert_eq!(detected_env.executable, Some(python_exe));
    assert_eq!(detected_env.prefix, Some(python_install_dir));
    
    // Test the find method
    let reporter = TestReporter::new();
    uv_locator.find(&reporter);
    
    let found_envs = reporter.get_environments();
    assert_eq!(found_envs.len(), 1);
    assert_eq!(found_envs[0].kind, Some(pet_core::python_environment::PythonEnvironmentKind::Uv));
}

#[test]
fn test_uv_locator_non_uv_environment() {
    let env = TestEnvironment::new();
    let uv_locator = Uv::from(&env);
    
    // Test with a non-UV Python environment
    let python_env = PythonEnv {
        executable: PathBuf::from("/usr/bin/python"),
        prefix: Some(PathBuf::from("/usr")),
        version: None,
        symlinks: None,
    };
    
    let detected_env = uv_locator.try_from(&python_env);
    assert!(detected_env.is_none());
}