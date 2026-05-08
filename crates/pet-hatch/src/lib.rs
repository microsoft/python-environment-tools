// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Hatch (<https://hatch.pypa.io>) environment locator.
//!
//! Hatch creates standard PEP 405 virtual environments (with a `pyvenv.cfg`),
//! but stores them in a known nested layout under its data directory. The
//! default layout is:
//!
//! ```text
//! <data_dir>/env/virtual/<project_name>/<project_id>/<venv_name>/
//! ```
//!
//! where `<data_dir>` is the platform-specific Hatch data directory and
//! `<project_id>` is a hash of the project root path. This is exactly three
//! components deep relative to `<data_dir>/env/virtual` (see Hatch's
//! `src/hatch/env/virtual.py` — `app_virtual_env_path`).
//!
//! In addition, projects can configure a custom storage location via
//! `[tool.hatch.dirs.env]` in `pyproject.toml` or `[dirs.env]` in
//! `hatch.toml`, e.g.:
//!
//! ```toml
//! [tool.hatch.dirs.env]
//! virtual = ".hatch"
//! ```
//!
//! When the configured `virtual` path is relative or matches `~/.virtualenvs`,
//! Hatch uses a flat layout: `<configured_dir>/<venv_name>/`.

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
use serde::Deserialize;

/// Subdirectory under the Hatch data directory where the default
/// "virtual" environment storage lives.
///
/// See `EnvironmentInterface.isolated_data_directory` and the `virtual`
/// plugin's `PLUGIN_NAME` in Hatch's source.
const VIRTUAL_ENV_SUBDIR: [&str; 2] = ["env", "virtual"];

pub struct Hatch {
    /// Default storage directory for Hatch virtual environments — i.e.
    /// `<data_dir>/env/virtual`. Resolved at construction. None if the
    /// directory does not yet exist (it is created lazily by Hatch).
    default_virtual_dir: Option<PathBuf>,
    /// Workspace directories supplied via configuration. Used to discover
    /// project-local Hatch environments via parsed `dirs.env.virtual` config.
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
            default_virtual_dir: get_default_virtual_dir(environment),
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
                .and_then(Path::parent)
                .map(Path::to_path_buf)
        })?;

        // A pyvenv.cfg must be present — Hatch envs are always venvs.
        let cfg = PyVenvCfg::find(&prefix)?;

        // Case 1: prefix lives in the default `<data_dir>/env/virtual` storage,
        // exactly three components deep:
        //   <storage>/<project_name>/<project_id>/<venv_name>
        if let Some(storage) = self.default_virtual_dir.as_deref() {
            if let Some(env_name) = match_default_storage_layout(&prefix, storage) {
                trace!(
                    "Hatch env (default storage) {} found at {}",
                    env_name,
                    env.executable.display()
                );
                return Some(build_env(&prefix, &cfg, env_name, None, &env.executable));
            }
        }

        // Case 2: prefix lives one level under a workspace's configured
        // `dirs.env.virtual` directory (flat layout).
        let workspaces = self
            .workspace_directories
            .lock()
            .expect("workspace_directories mutex poisoned")
            .clone();
        for workspace in &workspaces {
            for virtual_dir in resolve_project_virtual_dirs(workspace) {
                if prefix_is_directly_under(&prefix, &virtual_dir) {
                    let env_name = prefix
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    trace!(
                        "Hatch env (project-local) {} found at {}",
                        env_name,
                        env.executable.display()
                    );
                    return Some(build_env(
                        &prefix,
                        &cfg,
                        env_name,
                        Some(workspace.clone()),
                        &env.executable,
                    ));
                }
            }
        }

        None
    }

    fn find(&self, reporter: &dyn Reporter) {
        // 1. Walk the default storage directory.
        if let Some(storage) = self.default_virtual_dir.as_deref() {
            for env in find_envs_in_default_storage(storage) {
                reporter.report_environment(&env);
            }
        }

        // 2. Walk project-local virtual directories for each configured workspace.
        let workspaces = self
            .workspace_directories
            .lock()
            .expect("workspace_directories mutex poisoned")
            .clone();
        for workspace in &workspaces {
            for virtual_dir in resolve_project_virtual_dirs(workspace) {
                for env in find_envs_in_flat_dir(&virtual_dir, Some(workspace.clone())) {
                    reporter.report_environment(&env);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Hatch data directory resolution
// ---------------------------------------------------------------------------

/// Resolves `<data_dir>/env/virtual`, the directory Hatch uses for its
/// `virtual` environment plugin by default.
///
/// Resolution order matches Hatch itself:
/// 1. `HATCH_DATA_DIR` env var (then append `env/virtual`).
/// 2. Platform default for `platformdirs.user_data_dir("hatch", appauthor=False)`
///    (then append `env/virtual`).
///
/// Returns `None` if the resulting directory does not exist on disk.
fn get_default_virtual_dir(environment: &dyn Environment) -> Option<PathBuf> {
    // If HATCH_DATA_DIR is set and non-empty, Hatch *only* uses that location —
    // it never falls back to the platform default. Mirror that behaviour: return
    // the env/virtual subdir when it exists on disk, otherwise None. Do not
    // fall through to platform defaults, or we'd risk attributing platform-
    // default envs to Hatch when the user has redirected Hatch elsewhere.
    if let Some(custom) = environment.get_env_var("HATCH_DATA_DIR".to_string()) {
        if !custom.is_empty() {
            let path = append_virtual_subdir(PathBuf::from(custom));
            return if path.is_dir() {
                Some(norm_case(path))
            } else {
                None
            };
        }
    }
    let path = append_virtual_subdir(platform_default_data_dir(environment)?);
    if path.is_dir() {
        Some(norm_case(path))
    } else {
        None
    }
}

fn append_virtual_subdir(data_dir: PathBuf) -> PathBuf {
    let mut path = data_dir;
    for segment in VIRTUAL_ENV_SUBDIR {
        path.push(segment);
    }
    path
}

/// Platform default for Hatch's data directory.
///
/// Mirrors `platformdirs.user_data_dir("hatch", appauthor=False)`.
#[cfg(target_os = "linux")]
fn platform_default_data_dir(environment: &dyn Environment) -> Option<PathBuf> {
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
fn platform_default_data_dir(environment: &dyn Environment) -> Option<PathBuf> {
    Some(
        environment
            .get_user_home()?
            .join("Library")
            .join("Application Support")
            .join("hatch"),
    )
}

#[cfg(target_os = "windows")]
fn platform_default_data_dir(environment: &dyn Environment) -> Option<PathBuf> {
    // Windows: %USERPROFILE%\AppData\Local\hatch (matches platformdirs with
    // appauthor=False). Equivalent to %LOCALAPPDATA%\hatch when LOCALAPPDATA
    // is set, which is the common case.
    if let Some(local) = environment.get_env_var("LOCALAPPDATA".to_string()) {
        if !local.is_empty() {
            return Some(PathBuf::from(local).join("hatch"));
        }
    }
    Some(
        environment
            .get_user_home()?
            .join("AppData")
            .join("Local")
            .join("hatch"),
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_default_data_dir(environment: &dyn Environment) -> Option<PathBuf> {
    Some(
        environment
            .get_user_home()?
            .join(".local")
            .join("share")
            .join("hatch"),
    )
}

// ---------------------------------------------------------------------------
// Layout matching
// ---------------------------------------------------------------------------

/// If `prefix` lives exactly three components deep under `storage`
/// (i.e. `<storage>/<project_name>/<project_id>/<venv_name>`), return the
/// final component (`<venv_name>`).
fn match_default_storage_layout(prefix: &Path, storage: &Path) -> Option<String> {
    let normalized = norm_case(prefix);
    let rel = normalized.strip_prefix(storage).ok()?;
    let parts: Vec<_> = rel.iter().collect();
    if parts.len() == 3 {
        Some(parts[2].to_string_lossy().to_string())
    } else {
        None
    }
}

/// True iff `prefix`'s parent equals `dir` (case-insensitive on Windows).
fn prefix_is_directly_under(prefix: &Path, dir: &Path) -> bool {
    match prefix.parent() {
        Some(parent) => norm_case(parent) == norm_case(dir),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Project config (pyproject.toml / hatch.toml) parsing
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
struct PyProject {
    tool: Option<PyProjectTool>,
}

#[derive(Deserialize, Default)]
struct PyProjectTool {
    hatch: Option<HatchConfig>,
}

#[derive(Deserialize, Default)]
struct HatchConfig {
    dirs: Option<HatchDirs>,
}

#[derive(Deserialize, Default)]
struct HatchDirs {
    env: Option<toml::value::Table>,
}

/// Read the configured `dirs.env.virtual` paths for a workspace and resolve
/// each to an absolute directory. Both `pyproject.toml` (`[tool.hatch.dirs.env]`)
/// and a top-level `hatch.toml` (`[dirs.env]`) are checked.
///
/// Each value may be relative (resolved against the workspace root) or
/// absolute. Returns an empty Vec if the workspace is not a Hatch project,
/// or if no `virtual` value is configured.
fn resolve_project_virtual_dirs(workspace: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for raw in read_configured_virtual_paths(workspace) {
        let resolved = if Path::new(&raw).is_absolute() {
            PathBuf::from(&raw)
        } else {
            workspace.join(&raw)
        };
        if resolved.is_dir() {
            dirs.push(norm_case(resolved));
        }
    }
    dirs
}

fn read_configured_virtual_paths(workspace: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    // pyproject.toml: [tool.hatch.dirs.env]
    if let Ok(contents) = fs::read_to_string(workspace.join("pyproject.toml")) {
        if let Ok(pyproject) = toml::from_str::<PyProject>(&contents) {
            if let Some(virtual_value) = pyproject
                .tool
                .and_then(|t| t.hatch)
                .and_then(|h| h.dirs)
                .and_then(|d| d.env)
                .and_then(|env| env.get("virtual").cloned())
                .and_then(|v| v.as_str().map(str::to_string))
            {
                paths.push(virtual_value);
            }
        }
    }
    // hatch.toml: [dirs.env]
    if let Ok(contents) = fs::read_to_string(workspace.join("hatch.toml")) {
        if let Ok(hatch) = toml::from_str::<HatchConfig>(&contents) {
            if let Some(virtual_value) = hatch
                .dirs
                .and_then(|d| d.env)
                .and_then(|env| env.get("virtual").cloned())
                .and_then(|v| v.as_str().map(str::to_string))
            {
                paths.push(virtual_value);
            }
        }
    }
    paths
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Walk `<storage>/<project_name>/<project_id>/<venv_name>/` and report
/// each leaf venv discovered.
fn find_envs_in_default_storage(storage: &Path) -> Vec<PythonEnvironment> {
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
        let id_dirs = match fs::read_dir(&project_dir) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for id_entry in id_dirs.filter_map(Result::ok) {
            let id_dir = id_entry.path();
            if !id_dir.is_dir() {
                continue;
            }
            let env_dirs = match fs::read_dir(&id_dir) {
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
    }
    envs
}

/// Walk `<dir>/<venv_name>/` and report each venv discovered.
fn find_envs_in_flat_dir(dir: &Path, project: Option<PathBuf>) -> Vec<PythonEnvironment> {
    let mut envs = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(d) => d,
        Err(_) => return envs,
    };
    for entry in entries.filter_map(Result::ok) {
        let env_dir = entry.path();
        if !env_dir.is_dir() {
            continue;
        }
        if let Some(env) = build_env_from_prefix(&env_dir, project.clone()) {
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

fn build_env(
    prefix: &Path,
    cfg: &PyVenvCfg,
    fallback_name: String,
    project_path: Option<PathBuf>,
    executable: &Path,
) -> PythonEnvironment {
    let env_name = cfg.prompt.clone().unwrap_or(fallback_name);
    PythonEnvironmentBuilder::new(Some(PythonEnvironmentKind::Hatch))
        .name(Some(env_name))
        .executable(Some(executable.to_path_buf()))
        .version(cfg.version.clone())
        .prefix(Some(prefix.to_path_buf()))
        .symlinks(Some(find_executables(prefix)))
        .project(project_path)
        .build()
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

    fn write_pyvenv_cfg(prefix: &Path, prompt: &str, version: &str) {
        fs::create_dir_all(prefix).unwrap();
        fs::write(
            prefix.join("pyvenv.cfg"),
            format!("home = /usr/bin\nversion = {version}\nprompt = {prompt}\n"),
        )
        .unwrap();
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

    fn make_locator(default_virtual_dir: Option<PathBuf>) -> Hatch {
        Hatch {
            default_virtual_dir,
            workspace_directories: Arc::new(Mutex::new(vec![])),
        }
    }

    #[test]
    fn kind_and_supported_categories() {
        let locator = make_locator(None);
        assert_eq!(locator.get_kind(), LocatorKind::Hatch);
        assert_eq!(
            locator.supported_categories(),
            vec![PythonEnvironmentKind::Hatch]
        );
    }

    #[test]
    fn try_from_identifies_env_in_default_storage_three_levels_deep() {
        // Layout: <storage>/<project_name>/<project_id>/<venv_name>
        let temp = TempDir::new().unwrap();
        let storage = temp.path().join("env").join("virtual");
        let prefix = storage.join("myproj").join("ABCDEF12").join("default");
        write_pyvenv_cfg(&prefix, "default", "3.12.1");
        let exe = write_python_exe(&prefix);

        let locator = make_locator(Some(norm_case(&storage)));
        let env = PythonEnv::new(exe, Some(prefix.clone()), None);
        let identified = locator.try_from(&env).expect("Hatch env should match");
        assert_eq!(identified.kind, Some(PythonEnvironmentKind::Hatch));
        assert_eq!(identified.name, Some("default".to_string()));
        assert_eq!(identified.version, Some("3.12.1".to_string()));
        assert_eq!(identified.prefix, Some(norm_case(&prefix)));
        assert!(identified.project.is_none());
    }

    #[test]
    fn try_from_rejects_two_levels_deep_under_storage() {
        // PR #451's broken assumption: only 2 components deep.
        let temp = TempDir::new().unwrap();
        let storage = temp.path().join("env").join("virtual");
        let prefix = storage.join("myproj-hash").join("default");
        write_pyvenv_cfg(&prefix, "default", "3.12.1");
        let exe = write_python_exe(&prefix);

        let locator = make_locator(Some(norm_case(&storage)));
        let env = PythonEnv::new(exe, Some(prefix), None);
        assert!(locator.try_from(&env).is_none());
    }

    #[test]
    fn try_from_rejects_four_levels_deep_under_storage() {
        let temp = TempDir::new().unwrap();
        let storage = temp.path().join("env").join("virtual");
        let prefix = storage.join("a").join("b").join("c").join("d");
        write_pyvenv_cfg(&prefix, "d", "3.12.1");
        let exe = write_python_exe(&prefix);

        let locator = make_locator(Some(norm_case(&storage)));
        let env = PythonEnv::new(exe, Some(prefix), None);
        assert!(locator.try_from(&env).is_none());
    }

    #[test]
    fn try_from_returns_none_for_plain_venv() {
        let temp = TempDir::new().unwrap();
        let prefix = temp.path().join(".venv");
        write_pyvenv_cfg(&prefix, "venv", "3.12.1");
        let exe = write_python_exe(&prefix);

        let locator = make_locator(Some(temp.path().join("nonexistent")));
        let env = PythonEnv::new(exe, Some(prefix), None);
        assert!(locator.try_from(&env).is_none());
    }

    #[test]
    fn try_from_identifies_project_local_env_via_pyproject() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("pyproject.toml"),
            b"[project]\nname = \"foo\"\n\n[tool.hatch.dirs.env]\nvirtual = \".hatch\"\n",
        )
        .unwrap();
        let virtual_dir = project.join(".hatch");
        let prefix = virtual_dir.join("default");
        write_pyvenv_cfg(&prefix, "default", "3.11.0");
        let exe = write_python_exe(&prefix);

        let locator = Hatch {
            default_virtual_dir: None,
            workspace_directories: Arc::new(Mutex::new(vec![project.clone()])),
        };
        let env = PythonEnv::new(exe, Some(prefix), None);
        let identified = locator.try_from(&env).expect("project-local env match");
        assert_eq!(identified.kind, Some(PythonEnvironmentKind::Hatch));
        assert_eq!(identified.project, Some(norm_case(&project)));
        assert_eq!(identified.name, Some("default".to_string()));
    }

    #[test]
    fn try_from_identifies_project_local_env_via_hatch_toml() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("hatch.toml"),
            b"[dirs.env]\nvirtual = \".hatch\"\n",
        )
        .unwrap();
        let prefix = project.join(".hatch").join("default");
        write_pyvenv_cfg(&prefix, "default", "3.11.0");
        let exe = write_python_exe(&prefix);

        let locator = Hatch {
            default_virtual_dir: None,
            workspace_directories: Arc::new(Mutex::new(vec![project.clone()])),
        };
        let env = PythonEnv::new(exe, Some(prefix), None);
        let identified = locator.try_from(&env).expect("project-local env match");
        assert_eq!(identified.project, Some(norm_case(&project)));
    }

    #[test]
    fn try_from_rejects_project_local_without_dirs_env_config() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        // pyproject.toml is present but does not configure dirs.env.virtual.
        fs::write(
            project.join("pyproject.toml"),
            b"[project]\nname = \"foo\"\n[tool.hatch.envs.default]\n",
        )
        .unwrap();
        let prefix = project.join(".hatch").join("default");
        write_pyvenv_cfg(&prefix, "default", "3.11.0");
        let exe = write_python_exe(&prefix);

        let locator = Hatch {
            default_virtual_dir: None,
            workspace_directories: Arc::new(Mutex::new(vec![project])),
        };
        let env = PythonEnv::new(exe, Some(prefix), None);
        assert!(locator.try_from(&env).is_none());
    }

    #[test]
    fn find_reports_envs_in_default_storage() {
        let temp = TempDir::new().unwrap();
        let storage = temp.path().join("env").join("virtual");
        for env_name in ["default", "test"] {
            let prefix = storage.join("myproj").join("AbCdEf12").join(env_name);
            write_pyvenv_cfg(&prefix, env_name, "3.12.1");
            write_python_exe(&prefix);
        }
        // A bogus shallower entry should be ignored (no pyvenv.cfg here).
        fs::create_dir_all(storage.join("orphan")).unwrap();

        let envs = find_envs_in_default_storage(&storage);
        assert_eq!(envs.len(), 2);
        for env in envs {
            assert_eq!(env.kind, Some(PythonEnvironmentKind::Hatch));
            assert_eq!(env.version.as_deref(), Some("3.12.1"));
        }
    }

    #[test]
    fn find_reports_project_local_envs() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("proj");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("pyproject.toml"),
            b"[tool.hatch.dirs.env]\nvirtual = \".hatch\"\n",
        )
        .unwrap();
        let prefix = project.join(".hatch").join("default");
        write_pyvenv_cfg(&prefix, "default", "3.11.0");
        write_python_exe(&prefix);

        let virtual_dirs = resolve_project_virtual_dirs(&project);
        assert_eq!(virtual_dirs.len(), 1);
        let envs = find_envs_in_flat_dir(&virtual_dirs[0], Some(project.clone()));
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].project, Some(norm_case(&project)));
    }

    #[test]
    fn resolve_project_virtual_dirs_skips_non_hatch_projects() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("proj");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("pyproject.toml"),
            b"[project]\nname = \"foo\"\n",
        )
        .unwrap();
        assert!(resolve_project_virtual_dirs(&project).is_empty());
    }

    #[test]
    fn resolve_project_virtual_dirs_supports_absolute_path() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("proj");
        fs::create_dir_all(&project).unwrap();
        let absolute = temp.path().join("custom-envs");
        fs::create_dir_all(&absolute).unwrap();
        fs::write(
            project.join("pyproject.toml"),
            format!(
                "[tool.hatch.dirs.env]\nvirtual = \"{}\"\n",
                absolute.display().to_string().replace('\\', "\\\\")
            ),
        )
        .unwrap();

        let dirs = resolve_project_virtual_dirs(&project);
        assert_eq!(dirs, vec![norm_case(&absolute)]);
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
        assert_eq!(
            platform_default_data_dir(&env),
            Some(temp.path().join("hatch"))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn data_dir_falls_back_to_local_share_on_linux() {
        let env = TestEnv {
            home: Some(PathBuf::from("/home/test")),
            vars: HashMap::new(),
        };
        assert_eq!(
            platform_default_data_dir(&env),
            Some(PathBuf::from("/home/test/.local/share/hatch"))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn data_dir_uses_application_support_on_macos() {
        let env = TestEnv {
            home: Some(PathBuf::from("/Users/test")),
            vars: HashMap::new(),
        };
        assert_eq!(
            platform_default_data_dir(&env),
            Some(PathBuf::from(
                "/Users/test/Library/Application Support/hatch"
            ))
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn data_dir_uses_localappdata_on_windows() {
        let mut vars = HashMap::new();
        vars.insert(
            "LOCALAPPDATA".to_string(),
            "C:\\Users\\test\\AppData\\Local".to_string(),
        );
        let env = TestEnv {
            home: Some(PathBuf::from("C:\\Users\\test")),
            vars,
        };
        assert_eq!(
            platform_default_data_dir(&env),
            Some(PathBuf::from("C:\\Users\\test\\AppData\\Local\\hatch"))
        );
    }

    #[test]
    fn default_virtual_dir_honours_hatch_data_dir_env_var() {
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
        assert_eq!(get_default_virtual_dir(&env), Some(norm_case(virt)));
    }

    #[test]
    fn default_virtual_dir_does_not_fall_back_when_hatch_data_dir_is_set() {
        // If HATCH_DATA_DIR is set, Hatch only uses that location. We must
        // never silently fall through to the platform default — that could
        // misattribute platform-default envs to Hatch when the user has
        // redirected Hatch elsewhere.
        let temp = TempDir::new().unwrap();
        // Set HATCH_DATA_DIR to a directory whose env/virtual subdir does not exist.
        let mut vars = HashMap::new();
        vars.insert(
            "HATCH_DATA_DIR".to_string(),
            temp.path().to_string_lossy().to_string(),
        );
        let env = TestEnv {
            home: Some(temp.path().to_path_buf()),
            vars,
        };
        assert_eq!(get_default_virtual_dir(&env), None);
    }
}
