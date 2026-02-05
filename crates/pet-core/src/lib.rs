// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::path::PathBuf;

use env::PythonEnv;
use manager::EnvManager;
use python_environment::{PythonEnvironment, PythonEnvironmentKind};
use reporter::Reporter;

pub mod arch;
pub mod cache;
pub mod env;
pub mod manager;
pub mod os_environment;
pub mod python_environment;
pub mod pyvenv_cfg;
pub mod reporter;
pub mod telemetry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatorResult {
    pub managers: Vec<EnvManager>,
    pub environments: Vec<PythonEnvironment>,
}

#[derive(Debug, Default, Clone)]
pub struct Configuration {
    /// These are paths like workspace folders, where we can look for environments.
    pub workspace_directories: Option<Vec<PathBuf>>,
    pub executables: Option<Vec<PathBuf>>,
    pub conda_executable: Option<PathBuf>,
    pub pipenv_executable: Option<PathBuf>,
    pub poetry_executable: Option<PathBuf>,
    /// Custom locations where environments can be found.
    /// These are different from search_paths, as these are specific directories where environments are expected.
    /// environment_directories on the other hand can be any directory such as a workspace folder, where envs might never exist.
    pub environment_directories: Option<Vec<PathBuf>>,
    /// Directory to cache the Python environment details.
    pub cache_directory: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LocatorKind {
    Conda,
    Homebrew,
    LinuxGlobal,
    MacCommandLineTools,
    MacPythonOrg,
    MacXCode,
    PipEnv,
    Pixi,
    Poetry,
    PyEnv,
    Uv,
    Venv,
    VirtualEnv,
    VirtualEnvWrapper,
    WinPython,
    WindowsRegistry,
    WindowsStore,
}

pub trait Locator: Send + Sync {
    /// Returns the name of the locator.
    fn get_kind(&self) -> LocatorKind;
    /// Configures the locator with the given configuration.
    ///
    /// Override this method if you need to store configuration in the locator.
    ///
    /// # Why `&self` instead of `&mut self`?
    ///
    /// Locators are shared across threads via `Arc<dyn Locator>` and may be
    /// configured while other operations are in progress. Using `&self` allows
    /// concurrent access without requiring the caller to hold an exclusive lock
    /// on the entire locator.
    ///
    /// Implementations that need to store configuration should use interior
    /// mutability (e.g., `Mutex<T>` or `RwLock<T>`) for the mutable fields only.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::sync::Mutex;
    /// use std::path::PathBuf;
    ///
    /// struct MyLocator {
    ///     workspace_dirs: Mutex<Vec<PathBuf>>,
    /// }
    ///
    /// impl Locator for MyLocator {
    ///     fn configure(&self, config: &Configuration) {
    ///         if let Some(dirs) = &config.workspace_directories {
    ///             *self.workspace_dirs.lock().expect("workspace_dirs mutex poisoned") = dirs.clone();
    ///         }
    ///     }
    ///     // ... other required methods
    /// }
    /// ```
    fn configure(&self, _config: &Configuration) {
        //
    }
    /// Returns a list of supported categories for this locator.
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind>;
    /// Given a Python executable, and some optional data like prefix,
    /// this method will attempt to convert it to a PythonEnvironment that can be supported by this particular locator.
    /// If an environment is not supported by this locator, then None is returned.
    ///
    /// Note: The returned environment could have some missing information.
    /// This is because the `from` will do a best effort to get the environment information without spawning Python.
    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment>;
    /// Finds all environments specific to this locator.
    fn find(&self, reporter: &dyn Reporter);
}
