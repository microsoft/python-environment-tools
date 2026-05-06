// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Hatch environment locator.
//!
//! Hatch (<https://hatch.pypa.io>) creates standard PEP 405 virtual environments
//! (with a `pyvenv.cfg`), but stores them in a known layout that allows us to
//! distinguish them from generic venvs. By default, Hatch stores environments in:
//!
//! ```text
//! <data_dir>/env/virtual/<project-hash>/<env-name>/
//! ```
//!
//! where `<data_dir>` is the platform-specific Hatch data directory (see
//! [`hatch_data_dir`]) and `<project-hash>` encodes the originating project's
//! path. Hatch sets the `prompt` field in `pyvenv.cfg` to the environment name.
//!
//! Projects may also be configured to keep environments under a project-local
//! `.hatch/` directory (via `path = ".hatch"` in `hatch.toml` or
//! `[tool.hatch.envs.*]`). In that case the layout is
//! `<project>/.hatch/<env-name>/`.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use log::trace;
use pet_core::{
    env::PythonEnv,
    os_environment::Environment,
    python_environment::{PythonEnvironment, PythonEnvironmentBuilder, PythonEnvironmentKind},
    pyvenv_cfg::PyVenvCfg,
    reporter::Reporter,
    Configuration, Locator, LocatorKind, RefreshStatePersistence,
};
use pet_fs::path::norm_case;
use pet_python_utils::executable::{find_executable, find_executables};

/// Subdirectory under the Hatch data directory where the default
/// "virtual" environment storage lives.
const VIRTUAL_ENV_SUBDIR: &[&str] = &["env", "virtual"];

/// Conventional name of the project-local Hatch environment directory.
const PROJECT_LOCAL_DIR: &str = ".hatch";

pub struct Hatch {
    /// Directory where Hatch stores managed virtual environments,
    /// e.g. `~/.local/share/hatch/env/virtual` on Linux.
    virtual_storage_dir: Option<PathBuf>,
    /// Workspace directories supplied via configuration. Used to discover
    /// project-local `.hatch/` environments.
    workspace_directories: Arc<Mutex<Vec<PathBuf>>>,
}

impl Default for Hatch {
    fn default() -> Self {
        Self::from(&pet_core::os_environment::EnvironmentApi::new())
    }
}

impl Hatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from(environment: &dyn Environment) -> Self {
        Self {
            virtual_storage_dir: get_hatch_virtual_storage_dir(environment),
            workspace_directories: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Locator for Hatch {
    fn get_kind(&self) -> LocatorKind {
        LocatorKind::Hatch
    }

    fn refresh_state(&self) -> RefreshStatePersistence {
        RefreshStatePersistence::ConfiguredOnly
    }

    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> {
        vec![PythonEnvironmentKind::Hatch]
    }

    fn configure(&self, config: &Configuration) {
        let mut ws = self
            .workspace_directories
            .lock()
            .expect("workspace_directories mutex poisoned");
        ws.clear();
        if let Some(dirs) = config.workspace_directories.as_ref() {
            ws.extend(dirs.iter().cloned());
        }
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        // Determine the prefix (sysprefix) of this environment.
        let prefix = env.prefix.clone().or_else(|| {
            env.executable
                .parent()
                .and_then(|p| p.parent().map(Path::to_path_buf))
        })?;

        let (project_path, env_name) =
            classify_hatch_prefix(&prefix, self.virtual_storage_dir.as_deref())?;

        // A pyvenv.cfg must be present for this to be a valid venv created by Hatch.
        let cfg = PyVenvCfg::find(&prefix)?;
        // Hatch always writes a `prompt` field; treat its absence as a stronger
        // signal that this isn't actually a Hatch-managed env, only when the
        // env lives in a path-shape we can't otherwise verify (project-local).
        // For the default virtual storage, the path itself is authoritative.
        let env_name = cfg.prompt.clone().unwrap_or(env_name);

        trace!(
            "Hatch env {} found at {}",
            env_name,
            env.executable.display()
        );

        Some(
            PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::Hatch))
                .name(Some(env_name))
                .executable(Some(env.executable.clone()))
                .version(env.version.clone().or(cfg.version))
                .symlinks(Some(find_executables(&prefix)))
                .prefix(Some(prefix))
                .project(project_path)
                .build(),
        )
    }

    fn find(&self, reporter: &dyn Reporter) {
        // 1. Discover environments under the default virtual storage dir.
        if let Some(ref storage) = self.virtual_storage_dir {
            for env in find_envs_in_virtual_storage(storage) {
                reporter.report_environment(&env);
            }
        }

        // 2. Discover project-local `.hatch/` environments under each workspace dir.
        let workspaces = self
            .workspace_directories
            .lock()
            .expect("workspace_directories mutex poisoned")
            .clone();
        for workspace in &workspaces {
            for env in find_project_local_envs(workspace) {
                reporter.report_environment(&env);
            }
        }
    }
}

/// Determine where Hatch stores its managed virtual environments.
///
/// Resolution order:
/// 1. `HATCH_DATA_DIR` env var (then append `env/virtual`).
/// 2. Platform-specific platformdirs default for `hatch` (with no app-author),
///    then append `env/virtual`.
fn get_hatch_virtual_storage_dir(environment: &dyn Environment) -> Option<PathBuf> {
    if let Some(custom) = environment.get_env_var("HATCH_DATA_DIR".to_string()) {
        let path = build_virtual_subdir(PathBuf::from(custom));
        if path.is_dir() {
            return Some(norm_case(path));
        }
    }
    let data_dir = hatch_data_dir(environment)?;
    let path = build_virtual_subdir(data_dir);
    if path.is_dir() {
        Some(norm_case(path))
    } else {
        None
    }
}

fn build_virtual_subdir(data_dir: PathBuf) -> PathBuf {
    let mut path = data_dir;
    for segment in VIRTUAL_ENV_SUBDIR {
        path.push(segment);
    }
    path
}

/// Returns the platform default Hatch data directory.
///
/// Mirrors `platformdirs.user_data_dir("hatch", appauthor=False)` which is the
/// behavior used by the Hatch CLI itself.
#[cfg(target_os = "linux")]
fn hatch_data_dir(environment: &dyn Environment) -> Option<PathBuf> {
    if let Some(xdg) = environment.get_env_var("XDG_DATA_HOME".to_string()) {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("hatch"));
        }
    }
    Some(
        environment
            .get_user_home()?
            .join(".local")
            .join("share")
            .join("hatch"),
    )
}

#[cfg(target_os = "macos")]
fn hatch_data_dir(environment: &dyn Environment) -> Option<PathBuf> {
    Some(
        environment
            .get_user_home()?
            .join("Library")
            .join("Application Support")
            .join("hatch"),
    )
}

#[cfg(target_os = "windows")]
fn hatch_data_dir(environment: &dyn Environment) -> Option<PathBuf> {
    let local_app_data = environment.get_env_var("LOCALAPPDATA".to_string())?;
    Some(PathBuf::from(local_app_data).join("hatch"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn hatch_data_dir(environment: &dyn Environment) -> Option<PathBuf> {
    Some(
        environment
            .get_user_home()?
            .join(".local")
            .join("share")
            .join("hatch"),
    )
}

/// Classify whether a given prefix is a Hatch environment, returning the
/// inferred project path (if known) and a default name for the env.
///
/// Returns `Some((project_path, env_name))` when the prefix is recognised as
/// a Hatch environment, and `None` otherwise.
fn classify_hatch_prefix(
    prefix: &Path,
    virtual_storage_dir: Option<&Path>,
) -> Option<(Option<PathBuf>, String)> {
    // Case 1: default virtual storage layout: <storage>/<project-hash>/<env-name>
    if let Some(storage) = virtual_storage_dir {
        if let Ok(rel) = prefix.strip_prefix(storage) {
            let parts: Vec<_> = rel.iter().collect();
            // Must be exactly two components: project-hash and env-name.
            if parts.len() == 2 {
                let env_name = parts[1].to_string_lossy().to_string();
                return Some((None, env_name));
            }
        }
    }

    // Case 2: project-local .hatch/<env-name>
    let parent = prefix.parent()?;
    if parent.file_name().is_some_and(|n| n == PROJECT_LOCAL_DIR) {
        if let Some(project) = parent.parent() {
            if has_hatch_project_marker(project) {
                let env_name = prefix
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                return Some((Some(norm_case(project)), env_name));
            }
        }
    }

    None
}

/// Returns true if `project` has a Hatch project marker (`hatch.toml` or
/// `[tool.hatch]` in `pyproject.toml`).
fn has_hatch_project_marker(project: &Path) -> bool {
    if project.join("hatch.toml").is_file() {
        return true;
    }
    let pyproject = project.join("pyproject.toml");
    if let Ok(contents) = fs::read_to_string(&pyproject) {
        // Lightweight check; we don't need a full TOML parser here. Hatch
        // configuration always lives under a `[tool.hatch]` table (or
        // sub-tables like `[tool.hatch.envs.default]`).
        for line in contents.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix('[') {
                if let Some(name) = rest.strip_suffix(']') {
                    let name = name.trim();
                    if name == "tool.hatch" || name.starts_with("tool.hatch.") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Walk `<storage>` and report all envs in the `<project-hash>/<env-name>` layout.
fn find_envs_in_virtual_storage(storage: &Path) -> Vec<PythonEnvironment> {
    let mut envs = Vec::new();
    let project_dirs = match fs::read_dir(storage) {
        Ok(d) => d,
        Err(_) => return envs,
    };
    for project_entry in project_dirs.filter_map(Result::ok) {
        let project_dir = project_entry.path();
        if !project_dir.is_dir() {
            continue;
        }
        let env_dirs = match fs::read_dir(&project_dir) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for env_entry in env_dirs.filter_map(Result::ok) {
            let env_dir = env_entry.path();
            if !env_dir.is_dir() {
                continue;
            }
            if let Some(env) = build_env_from_prefix(&env_dir, None) {
                envs.push(env);
            }
        }
    }
    envs
}

/// Walk `<workspace>/.hatch/` and report each environment found.
fn find_project_local_envs(workspace: &Path) -> Vec<PythonEnvironment> {
    let mut envs = Vec::new();
    let hatch_dir = workspace.join(PROJECT_LOCAL_DIR);
    if !hatch_dir.is_dir() {
        return envs;
    }
    if !has_hatch_project_marker(workspace) {
        // A bare `.hatch/` without any project marker is unlikely to belong to Hatch.
        return envs;
    }
    let entries = match fs::read_dir(&hatch_dir) {
        Ok(d) => d,
        Err(_) => return envs,
    };
    for entry in entries.filter_map(Result::ok) {
        let env_dir = entry.path();
        if !env_dir.is_dir() {
            continue;
        }
        if let Some(env) = build_env_from_prefix(&env_dir, Some(workspace.to_path_buf())) {
            envs.push(env);
        }
    }
    envs
}

fn build_env_from_prefix(
    prefix: &Path,
    project_path: Option<PathBuf>,
) -> Option<PythonEnvironment> {
    let cfg = PyVenvCfg::find(prefix)?;
    let executable = find_executable(prefix)?;
    let env_name = cfg
        .prompt
        .clone()
        .or_else(|| prefix.file_name().map(|n| n.to_string_lossy().to_string()));
    Some(
        PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::Hatch))
            .name(env_name)
            .executable(Some(executable))
            .version(cfg.version)
            .prefix(Some(prefix.to_path_buf()))
            .symlinks(Some(find_executables(prefix)))
            .project(project_path)
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    struct TestEnv {
        home: Option<PathBuf>,
        vars: HashMap<String, String>,
    }

    impl Environment for TestEnv {
        fn get_user_home(&self) -> Option<PathBuf> {
            self.home.clone()
        }
        fn get_root(&self) -> Option<PathBuf> {
            None
        }
        fn get_env_var(&self, key: String) -> Option<String> {
            self.vars.get(&key).cloned()
        }
        fn get_know_global_search_locations(&self) -> Vec<PathBuf> {
            vec![]
        }
    }

    fn write_pyvenv_cfg(prefix: &Path, prompt: &str, version: &str) -> PathBuf {
        fs::create_dir_all(prefix).unwrap();
        let cfg = prefix.join("pyvenv.cfg");
        fs::write(
            &cfg,
            format!("home = /usr/bin\nversion = {version}\nprompt = {prompt}\n"),
        )
        .unwrap();
        cfg
    }

    fn write_python_exe(prefix: &Path) -> PathBuf {
        let bin = prefix.join(if cfg!(windows) { "Scripts" } else { "bin" });
        fs::create_dir_all(&bin).unwrap();
        let exe = bin.join(if cfg!(windows) {
            "python.exe"
        } else {
            "python"
        });
        fs::write(&exe, b"").unwrap();
        exe
    }

    #[test]
    fn kind_and_supported_categories() {
        let locator = Hatch {
            virtual_storage_dir: None,
            workspace_directories: Arc::new(Mutex::new(vec![])),
        };
        assert_eq!(locator.get_kind(), LocatorKind::Hatch);
        assert_eq!(
            locator.supported_categories(),
            vec![PythonEnvironmentKind::Hatch]
        );
    }

    #[test]
    fn try_from_identifies_env_in_default_storage() {
        let temp = TempDir::new().unwrap();
        let storage = temp.path().join("env").join("virtual");
        let prefix = storage.join("myproj-AbCdEfGh").join("default");
        write_pyvenv_cfg(&prefix, "default", "3.12.1");
        let exe = write_python_exe(&prefix);

        let locator = Hatch {
            virtual_storage_dir: Some(norm_case(&storage)),
            workspace_directories: Arc::new(Mutex::new(vec![])),
        };
        let env = PythonEnv::new(exe.clone(), Some(prefix.clone()), None);
        let identified = locator.try_from(&env).unwrap();
        assert_eq!(identified.kind, Some(PythonEnvironmentKind::Hatch));
        assert_eq!(identified.name, Some("default".to_string()));
        assert_eq!(identified.version, Some("3.12.1".to_string()));
        assert_eq!(identified.prefix, Some(norm_case(&prefix)));
        // Project path is unknown for the default storage layout.
        assert!(identified.project.is_none());
    }

    #[test]
    fn try_from_returns_none_for_non_hatch_env() {
        let temp = TempDir::new().unwrap();
        let prefix = temp.path().join(".venv");
        write_pyvenv_cfg(&prefix, "venv", "3.12.1");
        let exe = write_python_exe(&prefix);

        let locator = Hatch {
            virtual_storage_dir: Some(temp.path().join("nonexistent")),
            workspace_directories: Arc::new(Mutex::new(vec![])),
        };
        let env = PythonEnv::new(exe, Some(prefix), None);
        assert!(locator.try_from(&env).is_none());
    }

    #[test]
    fn try_from_identifies_project_local_env() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        // Marker via hatch.toml.
        fs::write(project.join("hatch.toml"), b"[envs.default]\n").unwrap();
        let prefix = project.join(".hatch").join("default");
        write_pyvenv_cfg(&prefix, "default", "3.11.0");
        let exe = write_python_exe(&prefix);

        let locator = Hatch {
            virtual_storage_dir: None,
            workspace_directories: Arc::new(Mutex::new(vec![])),
        };
        let env = PythonEnv::new(exe, Some(prefix.clone()), None);
        let identified = locator.try_from(&env).unwrap();
        assert_eq!(identified.kind, Some(PythonEnvironmentKind::Hatch));
        assert_eq!(identified.name, Some("default".to_string()));
        assert_eq!(identified.project, Some(norm_case(&project)));
    }

    #[test]
    fn try_from_identifies_project_local_env_via_pyproject_marker() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("pyproject.toml"),
            b"[project]\nname = \"foo\"\n\n[tool.hatch.envs.default]\n",
        )
        .unwrap();
        let prefix = project.join(".hatch").join("default");
        write_pyvenv_cfg(&prefix, "default", "3.11.0");
        let exe = write_python_exe(&prefix);

        let locator = Hatch {
            virtual_storage_dir: None,
            workspace_directories: Arc::new(Mutex::new(vec![])),
        };
        let env = PythonEnv::new(exe, Some(prefix), None);
        let identified = locator.try_from(&env).unwrap();
        assert_eq!(identified.kind, Some(PythonEnvironmentKind::Hatch));
        assert_eq!(identified.project, Some(norm_case(&project)));
    }

    #[test]
    fn try_from_rejects_project_local_without_marker() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        // No hatch.toml or pyproject.toml marker.
        let prefix = project.join(".hatch").join("default");
        write_pyvenv_cfg(&prefix, "default", "3.11.0");
        let exe = write_python_exe(&prefix);

        let locator = Hatch {
            virtual_storage_dir: None,
            workspace_directories: Arc::new(Mutex::new(vec![])),
        };
        let env = PythonEnv::new(exe, Some(prefix), None);
        assert!(locator.try_from(&env).is_none());
    }

    #[test]
    fn try_from_rejects_wrong_depth_under_storage() {
        let temp = TempDir::new().unwrap();
        let storage = temp.path().join("env").join("virtual");
        // Only one component under storage (missing env-name).
        let prefix = storage.join("myproj-AbCdEfGh");
        write_pyvenv_cfg(&prefix, "default", "3.12.1");
        let exe = write_python_exe(&prefix);

        let locator = Hatch {
            virtual_storage_dir: Some(norm_case(&storage)),
            workspace_directories: Arc::new(Mutex::new(vec![])),
        };
        let env = PythonEnv::new(exe, Some(prefix), None);
        assert!(locator.try_from(&env).is_none());
    }

    #[test]
    fn find_reports_envs_in_virtual_storage() {
        let temp = TempDir::new().unwrap();
        let storage = temp.path().join("env").join("virtual");
        for name in ["default", "test"] {
            let prefix = storage.join("myproj-AbCdEfGh").join(name);
            write_pyvenv_cfg(&prefix, name, "3.12.1");
            write_python_exe(&prefix);
        }

        let envs = find_envs_in_virtual_storage(&storage);
        assert_eq!(envs.len(), 2);
        for env in envs {
            assert_eq!(env.kind, Some(PythonEnvironmentKind::Hatch));
        }
    }

    #[test]
    fn find_reports_project_local_envs() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("proj");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("hatch.toml"), b"").unwrap();
        let prefix = project.join(".hatch").join("default");
        write_pyvenv_cfg(&prefix, "default", "3.11.0");
        write_python_exe(&prefix);

        let envs = find_project_local_envs(&project);
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].project, Some(norm_case(&project)));
    }

    #[test]
    fn find_skips_project_local_without_marker() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("proj");
        let prefix = project.join(".hatch").join("default");
        write_pyvenv_cfg(&prefix, "default", "3.11.0");
        write_python_exe(&prefix);

        let envs = find_project_local_envs(&project);
        assert!(envs.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn data_dir_uses_xdg_data_home_when_set() {
        let temp = TempDir::new().unwrap();
        let mut vars = HashMap::new();
        vars.insert(
            "XDG_DATA_HOME".to_string(),
            temp.path().to_string_lossy().to_string(),
        );
        let env = TestEnv {
            home: Some(PathBuf::from("/home/test")),
            vars,
        };
        assert_eq!(hatch_data_dir(&env), Some(temp.path().join("hatch")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn data_dir_falls_back_to_local_share() {
        let env = TestEnv {
            home: Some(PathBuf::from("/home/test")),
            vars: HashMap::new(),
        };
        assert_eq!(
            hatch_data_dir(&env),
            Some(PathBuf::from("/home/test/.local/share/hatch"))
        );
    }

    #[test]
    fn virtual_storage_dir_honours_hatch_data_dir_env_var() {
        let temp = TempDir::new().unwrap();
        let virt = temp.path().join("env").join("virtual");
        fs::create_dir_all(&virt).unwrap();
        let mut vars = HashMap::new();
        vars.insert(
            "HATCH_DATA_DIR".to_string(),
            temp.path().to_string_lossy().to_string(),
        );
        let env = TestEnv {
            home: Some(temp.path().to_path_buf()),
            vars,
        };
        assert_eq!(get_hatch_virtual_storage_dir(&env), Some(norm_case(virt)));
    }
}
