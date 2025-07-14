// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::path::PathBuf;

use log::trace;
use pet_core::{
    env::PythonEnv,
    manager::{EnvManager, EnvManagerType},
    os_environment::Environment,
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind},
    reporter::Reporter,
    Locator, LocatorKind,
};
use pet_fs::path::norm_case;

pub struct Uv {
    environment: Option<Box<dyn Environment>>,
}

impl Uv {
    pub fn new() -> Self {
        Self { environment: None }
    }

    pub fn from(environment: &dyn Environment) -> Self {
        Self {
            environment: Some(Box::new(EnvironmentWrapper::from(environment))),
        }
    }

    /// Get the UV data directory where UV stores its data
    fn get_uv_data_dir(&self) -> Option<PathBuf> {
        if let Some(env) = &self.environment {
            // First check UV_DATA_DIR environment variable
            if let Some(uv_data_dir) = env.get_env_var("UV_DATA_DIR".to_string()) {
                return Some(PathBuf::from(uv_data_dir));
            }
        }

        // Fall back to platform-specific default locations
        if cfg!(windows) {
            // Windows: %APPDATA%\uv
            if let Some(env) = &self.environment {
                if let Some(appdata) = env.get_env_var("APPDATA".to_string()) {
                    return Some(PathBuf::from(appdata).join("uv"));
                }
            }
        } else {
            // Unix-like systems: ~/.local/share/uv
            if let Some(env) = &self.environment {
                if let Some(home) = env.get_env_var("HOME".to_string()) {
                    return Some(PathBuf::from(home).join(".local").join("share").join("uv"));
                }
            }
        }

        None
    }

    /// Get the directory where UV stores Python installations
    fn get_uv_python_dir(&self) -> Option<PathBuf> {
        self.get_uv_data_dir().map(|data_dir| data_dir.join("python"))
    }

    /// Check if a path might be a UV-managed Python installation
    fn is_uv_python(&self, env: &PythonEnv) -> bool {
        if let Some(uv_python_dir) = self.get_uv_python_dir() {
            let uv_python_dir = norm_case(uv_python_dir);
            
            // Check if the executable is under the UV python directory
            if let Some(exe_parent) = env.executable.parent() {
                let exe_parent = norm_case(exe_parent.to_path_buf());
                if exe_parent.starts_with(&uv_python_dir) {
                    return true;
                }
            }

            // Check if the prefix is under the UV python directory
            if let Some(prefix) = &env.prefix {
                let prefix = norm_case(prefix.clone());
                if prefix.starts_with(&uv_python_dir) {
                    return true;
                }
            }
        }

        false
    }

    /// Find UV executable in PATH
    fn find_uv_executable(&self) -> Option<PathBuf> {
        if let Some(env) = &self.environment {
            // Search for UV executable in known global locations
            for path in env.get_know_global_search_locations() {
                let uv_exe = if cfg!(windows) {
                    path.join("uv.exe")
                } else {
                    path.join("uv")
                };
                
                if uv_exe.exists() && uv_exe.is_file() {
                    return Some(uv_exe);
                }
            }
        }
        None
    }

    /// Discover UV-managed Python installations
    fn discover_uv_pythons(&self, reporter: &dyn Reporter) {
        let Some(uv_python_dir) = self.get_uv_python_dir() else {
            trace!("UV python directory not found");
            return;
        };

        if !uv_python_dir.exists() {
            trace!("UV python directory does not exist: {:?}", uv_python_dir);
            return;
        }

        trace!("Searching for UV Python installations in: {:?}", uv_python_dir);

        // Iterate through subdirectories in the UV python directory
        let Ok(entries) = std::fs::read_dir(&uv_python_dir) else {
            trace!("Failed to read UV python directory: {:?}", uv_python_dir);
            return;
        };

        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }

            // Look for Python executables in each installation directory
            let python_install_dir = entry.path();
            trace!("Checking UV Python installation: {:?}", python_install_dir);

            // UV typically installs Python in subdirectories like cpython-3.11.0-windows-x86_64-none
            // The executable is usually in bin/ (Unix) or Scripts/ (Windows) or directly in the root
            let possible_bin_dirs = if cfg!(windows) {
                vec!["Scripts", "bin", "."]
            } else {
                vec!["bin", "."]
            };

            for bin_dir_name in possible_bin_dirs {
                let bin_dir = python_install_dir.join(bin_dir_name);
                if !bin_dir.exists() {
                    continue;
                }

                // Look for Python executables
                let python_exe_names = if cfg!(windows) {
                    vec!["python.exe", "python3.exe"]
                } else {
                    vec!["python", "python3"]
                };

                for exe_name in python_exe_names {
                    let python_exe = bin_dir.join(exe_name);
                    if python_exe.exists() && python_exe.is_file() {
                        trace!("Found UV Python executable: {:?}", python_exe);

                        let env = PythonEnv {
                            executable: python_exe.clone(),
                            prefix: Some(python_install_dir.clone()),
                            version: None, // Will be determined later if needed
                            symlinks: None,
                        };

                        if let Some(python_env) = self.try_from(&env) {
                            reporter.report_environment(&python_env);
                        }
                        break; // Found an executable in this bin directory, move to next installation
                    }
                }
            }
        }
    }
}

impl Default for Uv {
    fn default() -> Self {
        Self::new()
    }
}

impl Locator for Uv {
    fn get_kind(&self) -> LocatorKind {
        LocatorKind::Uv
    }

    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::Uv]
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        if !self.is_uv_python(env) {
            return None;
        }

        trace!("Identified UV Python environment: {:?}", env.executable);

        let manager = self.find_uv_executable().map(|uv_exe| {
            EnvManager::new(uv_exe, EnvManagerType::Uv, None)
        });

        Some(
            PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::Uv))
                .executable(Some(env.executable.clone()))
                .prefix(env.prefix.clone())
                .version(env.version.clone())
                .manager(manager)
                .build(),
        )
    }

    fn find(&self, reporter: &dyn Reporter) {
        trace!("UV Locator: Starting search for UV Python installations");
        self.discover_uv_pythons(reporter);
    }
}

// Simple wrapper for Environment to make it work with the provided environment
struct EnvironmentWrapper {
    inner: Box<dyn Environment>,
}

impl EnvironmentWrapper {
    fn from(environment: &dyn Environment) -> Self {
        Self {
            inner: Box::new(SimpleEnvironment::new(environment)),
        }
    }
}

impl Environment for EnvironmentWrapper {
    fn get_user_home(&self) -> Option<PathBuf> {
        self.inner.get_user_home()
    }

    fn get_root(&self) -> Option<PathBuf> {
        self.inner.get_root()
    }

    fn get_env_var(&self, key: String) -> Option<String> {
        self.inner.get_env_var(key)
    }

    fn get_know_global_search_locations(&self) -> Vec<PathBuf> {
        self.inner.get_know_global_search_locations()
    }
}

// Simple environment that just forwards to system env vars for most cases
struct SimpleEnvironment {
    custom_vars: std::collections::HashMap<String, String>,
}

impl SimpleEnvironment {
    fn new(environment: &dyn Environment) -> Self {
        let mut custom_vars = std::collections::HashMap::new();
        
        // Try to get some common environment variables from the provided environment
        for key in ["UV_DATA_DIR", "HOME", "USERPROFILE", "APPDATA", "PATH"] {
            if let Some(value) = environment.get_env_var(key.to_string()) {
                custom_vars.insert(key.to_string(), value);
            }
        }
        
        Self { custom_vars }
    }
}

impl Environment for SimpleEnvironment {
    fn get_user_home(&self) -> Option<PathBuf> {
        self.custom_vars.get("HOME")
            .or_else(|| self.custom_vars.get("USERPROFILE"))
            .map(|h| PathBuf::from(h))
            .or_else(|| {
                std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .ok()
                    .map(PathBuf::from)
            })
    }

    fn get_root(&self) -> Option<PathBuf> {
        None
    }

    fn get_env_var(&self, key: String) -> Option<String> {
        self.custom_vars.get(&key).cloned()
            .or_else(|| std::env::var(key).ok())
    }

    fn get_know_global_search_locations(&self) -> Vec<PathBuf> {
        if let Some(path_var) = self.get_env_var("PATH".to_string()) {
            std::env::split_paths(&path_var)
                .filter(|p| p.exists())
                .collect()
        } else {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pet_core::os_environment::Environment;
    use std::collections::HashMap;

    struct MockEnvironment {
        env_vars: HashMap<String, String>,
    }

    impl MockEnvironment {
        fn new() -> Self {
            Self {
                env_vars: HashMap::new(),
            }
        }

        fn with_env_var(mut self, key: &str, value: &str) -> Self {
            self.env_vars.insert(key.to_string(), value.to_string());
            self
        }
    }

    impl Environment for MockEnvironment {
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
    fn test_get_uv_data_dir_custom() {
        let env = MockEnvironment::new()
            .with_env_var("UV_DATA_DIR", "/custom/uv/data");
        let uv = Uv::from(&env);
        
        assert_eq!(
            uv.get_uv_data_dir(),
            Some(PathBuf::from("/custom/uv/data"))
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_get_uv_data_dir_windows_default() {
        let env = MockEnvironment::new()
            .with_env_var("APPDATA", "C:\\Users\\test\\AppData\\Roaming");
        let uv = Uv::from(&env);
        
        assert_eq!(
            uv.get_uv_data_dir(),
            Some(PathBuf::from("C:\\Users\\test\\AppData\\Roaming\\uv"))
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_get_uv_data_dir_unix_default() {
        let env = MockEnvironment::new()
            .with_env_var("HOME", "/home/test");
        let uv = Uv::from(&env);
        
        assert_eq!(
            uv.get_uv_data_dir(),
            Some(PathBuf::from("/home/test/.local/share/uv"))
        );
    }

    #[test]
    fn test_get_uv_python_dir() {
        let env = MockEnvironment::new()
            .with_env_var("UV_DATA_DIR", "/test/uv");
        let uv = Uv::from(&env);
        
        assert_eq!(
            uv.get_uv_python_dir(),
            Some(PathBuf::from("/test/uv/python"))
        );
    }
}