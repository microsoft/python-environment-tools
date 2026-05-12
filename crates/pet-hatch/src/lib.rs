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
//! For these workspace-configured locations Hatch uses a flat layout:
//! `<configured_dir>/<venv_name>/`. Configured paths may be relative
//! (resolved against the workspace root), absolute, or use `~` /
//! `${HOME}` style expansion (e.g. `~/.virtualenvs`).

use std::{
    collections::HashSet,
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
use pet_fs::path::{expand_path, norm_case};
use pet_python_utils::executable::{find_executable, find_executables};
use serde::Deserialize;

/// Subdirectory under the Hatch data directory where the default
/// "virtual" environment storage lives.
///
/// See `EnvironmentInterface.isolated_data_directory` and the `virtual`
/// plugin's `PLUGIN_NAME` in Hatch's source.
const VIRTUAL_ENV_SUBDIR: [&str; 2] = ["env", "virtual"];

/// Per-workspace cache entry: workspace root, resolved
/// `dirs.env.virtual` paths, and the precomputed env-name allowlist.
///
/// `matcher` is used as a Hatch-specific guard when matching venvs in
/// workspace-configured `dirs.env.virtual` directories: a shared
/// directory like `~/.virtualenvs` can contain non-Hatch envs (created by
/// virtualenvwrapper, plain `venv`, etc.), so we only claim a venv if its
/// leaf directory name matches one of the env names declared in the
/// project's Hatch configuration. The matcher pre-normalizes names so the
/// `try_from()` hot path avoids per-call `to_lowercase()` / `format!()`
/// allocations over the allowlist.
struct WorkspaceEntry {
    workspace: PathBuf,
    virtual_dirs: Vec<PathBuf>,
    matcher: EnvNameMatcher,
}

/// Per-workspace cache populated by `configure()`. Entries are wrapped in
/// `Arc` so `find()` can snapshot the cache (clone the Vec of Arcs) and
/// release the lock cheaply before doing filesystem I/O — no deep
/// `Vec<PathBuf>` / matcher clone per call.
type WorkspaceVirtualDirs = Vec<Arc<WorkspaceEntry>>;

pub struct Hatch {
    /// Default storage directory for Hatch virtual environments — i.e.
    /// `<data_dir>/env/virtual`. Resolved at construction. The path may not
    /// exist on disk yet (Hatch creates it lazily on first use); existence
    /// is re-checked by `find()` at discovery time so envs created later in
    /// this process lifetime are still discoverable without a restart.
    /// `None` only when the platform data directory itself cannot be
    /// resolved (e.g. no home directory).
    default_virtual_dir: Option<PathBuf>,
    /// Per-workspace resolved virtual directories, computed during
    /// `configure()` so that hot-path identification (`try_from`) does no
    /// disk I/O or TOML parsing.
    workspace_virtual_dirs: Arc<Mutex<WorkspaceVirtualDirs>>,
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
            workspace_virtual_dirs: Arc::new(Mutex::new(Vec::new())),
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
        // Precompute and cache each workspace's resolved Hatch virtual dirs
        // and declared env names so `try_from()` does not have to re-read
        // or re-parse pyproject.toml / hatch.toml on every executable
        // identification attempt. We build the new cache *outside* the
        // lock to keep disk I/O out of the critical section.
        let mut new_cache: WorkspaceVirtualDirs = Vec::new();
        if let Some(dirs) = config.workspace_directories.as_ref() {
            for workspace in dirs {
                // Single parse of pyproject.toml + hatch.toml per workspace
                // — both `virtual_dirs` and `env_names` come from the same
                // TOML sections, so we read each file once here.
                let (virtual_dirs, env_names) = resolve_workspace_hatch_config(workspace);
                new_cache.push(Arc::new(WorkspaceEntry {
                    workspace: workspace.clone(),
                    virtual_dirs,
                    matcher: EnvNameMatcher::from_names(env_names),
                }));
            }
        }
        *self
            .workspace_virtual_dirs
            .lock()
            .expect("workspace_virtual_dirs mutex poisoned") = new_cache;
    }

    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> {
        // Determine the prefix (sysprefix) of this environment.
        let prefix = env.prefix.clone().or_else(|| {
            env.executable
                .parent()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
        })?;

        // Do the cheap path-shape classification *first* so we don't pay for
        // a `pyvenv.cfg` filesystem read on every non-Hatch venv that flows
        // through the locator chain.
        //
        // Case 1: prefix lives in the default `<data_dir>/env/virtual` storage,
        // exactly three components deep:
        //   <storage>/<project_name>/<project_id>/<venv_name>
        let mut classification: Option<(String, Option<PathBuf>)> = None;
        if let Some(storage) = self.default_virtual_dir.as_deref() {
            if let Some(env_name) = match_default_storage_layout(&prefix, storage) {
                classification = Some((env_name, None));
            }
        }

        // Case 2: prefix lives one level under a workspace's configured
        // `dirs.env.virtual` directory (flat layout). Inspect the cached
        // workspaces under the lock and capture the match instead of cloning
        // the entire cache.
        //
        // Because configured `dirs.env.virtual` may point at a shared
        // directory (e.g. `~/.virtualenvs`), we additionally require that
        // the venv's leaf directory name matches one of the env names
        // declared in the workspace's Hatch configuration. Otherwise an
        // unrelated virtualenvwrapper / `venv` env in the same directory
        // would be misclassified as Hatch-managed.
        if classification.is_none() {
            // Snapshot the cache (cheap `Arc` clones) under the lock and
            // release it before iterating, to keep `configure()` from being
            // blocked by callers on the hot identification path.
            let cache: Vec<Arc<WorkspaceEntry>> = self
                .workspace_virtual_dirs
                .lock()
                .expect("workspace_virtual_dirs mutex poisoned")
                .clone();
            'workspaces: for entry in cache.iter() {
                for virtual_dir in &entry.virtual_dirs {
                    if prefix_is_directly_under(&prefix, virtual_dir) {
                        let env_name = prefix
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if !entry.matcher.matches(&env_name) {
                            continue;
                        }
                        classification = Some((env_name, Some(entry.workspace.clone())));
                        break 'workspaces;
                    }
                }
            }
        }

        let (env_name, project_path) = classification?;

        // Now that we know this is (likely) a Hatch env, read pyvenv.cfg.
        // Hatch always writes one; if it's missing this isn't actually a
        // Hatch-managed env.
        let cfg = PyVenvCfg::find(&prefix)?;

        trace!(
            "Hatch env {} found at {}",
            env_name,
            env.executable.display()
        );
        Some(build_env(
            &prefix,
            &cfg,
            env_name,
            project_path,
            &env.executable,
        ))
    }

    fn find(&self, reporter: &dyn Reporter) {
        // 1. Walk the default storage directory if it currently exists. We
        //    re-check existence here (rather than caching the result of the
        //    check at construction) because the long-lived locator graph is
        //    built once at server startup; the user may install Hatch or
        //    create their first env after that point and we still want to
        //    discover it without a restart.
        if let Some(storage) = self.default_virtual_dir.as_deref() {
            if storage.is_dir() {
                for env in find_envs_in_default_storage(storage) {
                    reporter.report_environment(&env);
                }
            }
        }

        // 2. Walk project-local virtual directories for each configured workspace.
        //    Snapshot the cache (cheap `Arc` clones) under the lock, then
        //    release the lock before doing filesystem I/O. Apply the same
        //    env-name guard as `try_from()` so shared directories (e.g.
        //    `~/.virtualenvs`) only yield the workspace's declared envs.
        let workspaces: Vec<Arc<WorkspaceEntry>> = self
            .workspace_virtual_dirs
            .lock()
            .expect("workspace_virtual_dirs mutex poisoned")
            .clone();
        for entry in &workspaces {
            for virtual_dir in &entry.virtual_dirs {
                for env in find_envs_in_flat_dir(
                    virtual_dir,
                    Some(entry.workspace.clone()),
                    &entry.matcher,
                ) {
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
/// The returned path may not exist on disk yet; callers must check existence
/// at use time. This lets us correctly identify Hatch envs created later in
/// the same long-lived PET process without a restart.
fn get_default_virtual_dir(environment: &dyn Environment) -> Option<PathBuf> {
    // If HATCH_DATA_DIR is set and non-empty, Hatch *only* uses that location
    // — it never falls back to the platform default. Mirror that behaviour.
    // Do not fall through to platform defaults, or we'd risk attributing
    // platform-default envs to Hatch when the user has redirected Hatch
    // elsewhere.
    if let Some(custom) = environment.get_env_var("HATCH_DATA_DIR".to_string()) {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            // Expand ~ / ${HOME} / ${USERNAME} so a value like
            // `HATCH_DATA_DIR=~/.local/share/hatch` resolves to the user
            // home rather than a literal `~` directory.
            let expanded = expand_path(PathBuf::from(trimmed));
            // If the home directory is unavailable, `expand_path()` returns
            // the input verbatim. Don't normalize a leading `~` into a
            // literal directory under cwd — bail out so Hatch envs are not
            // attributed to a bogus path.
            if path_starts_with_tilde(&expanded) {
                return None;
            }
            return Some(norm_case(append_virtual_subdir(expanded)));
        }
    }
    Some(norm_case(append_virtual_subdir(platform_default_data_dir(
        environment,
    )?)))
}

fn append_virtual_subdir(data_dir: PathBuf) -> PathBuf {
    let mut path = data_dir;
    for segment in VIRTUAL_ENV_SUBDIR {
        path.push(segment);
    }
    path
}

/// Returns true if `path` still begins with a literal `~`, indicating that
/// `expand_path()` could not resolve the user's home directory (no HOME /
/// USERPROFILE set). Such paths must not be normalized or joined against
/// the workspace root, since `~` was not the user's intended directory.
fn path_starts_with_tilde(path: &Path) -> bool {
    path.to_str().is_some_and(|s| s.starts_with('~'))
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
    // Iterate components directly to avoid a per-call Vec allocation on the
    // identification hot path. We need exactly three components.
    let mut iter = rel.iter();
    let _project_name = iter.next()?;
    let _project_id = iter.next()?;
    let venv_name = iter.next()?;
    if iter.next().is_some() {
        return None;
    }
    Some(venv_name.to_string_lossy().to_string())
}

/// True iff `prefix`'s parent equals `dir` (case-insensitive on Windows).
///
/// `dir` is expected to be already normalized via `norm_case()` — entries
/// cached on the `Hatch` locator are normalized at `configure()`-time by
/// `resolve_virtual_paths_against_workspace()` (called from
/// `resolve_workspace_hatch_config()`), so we only normalize
/// `prefix.parent()` here — avoiding redundant `GetLongPathNameW` /
/// case-folding work on Windows in the identification hot path.
fn prefix_is_directly_under(prefix: &Path, dir: &Path) -> bool {
    match prefix.parent() {
        Some(parent) => norm_case(parent) == dir,
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
    envs: Option<toml::value::Table>,
}

#[derive(Deserialize, Default)]
struct HatchDirs {
    env: Option<toml::value::Table>,
}

/// Parse `pyproject.toml`'s `[tool.hatch]` table and `hatch.toml` (which
/// has the same shape as `HatchConfig`) for `workspace`, returning both
/// in a single pass. Returns `(pyproject_hatch, hatch_toml)` where each
/// is `None` if the corresponding file is missing or unparseable.
fn read_workspace_hatch_sections(workspace: &Path) -> (Option<HatchConfig>, Option<HatchConfig>) {
    let pyproject = fs::read_to_string(workspace.join("pyproject.toml"))
        .ok()
        .and_then(|s| toml::from_str::<PyProject>(&s).ok())
        .and_then(|pp| pp.tool)
        .and_then(|t| t.hatch);
    let hatch_toml = fs::read_to_string(workspace.join("hatch.toml"))
        .ok()
        .and_then(|s| toml::from_str::<HatchConfig>(&s).ok());
    (pyproject, hatch_toml)
}

fn extract_virtual_paths(sections: &(Option<HatchConfig>, Option<HatchConfig>)) -> Vec<String> {
    let mut paths = Vec::new();
    for section in [&sections.0, &sections.1].iter().copied().flatten() {
        if let Some(virtual_value) = section
            .dirs
            .as_ref()
            .and_then(|d| d.env.as_ref())
            .and_then(|env| env.get("virtual"))
            .and_then(|v| v.as_str().map(str::to_string))
        {
            paths.push(virtual_value);
        }
    }
    paths
}

fn extract_env_names(sections: &(Option<HatchConfig>, Option<HatchConfig>)) -> HashSet<String> {
    let mut names = HashSet::new();
    names.insert(HATCH_IMPLICIT_DEFAULT_ENV.to_string());
    for section in [&sections.0, &sections.1].iter().copied().flatten() {
        if let Some(envs) = section.envs.as_ref() {
            for key in envs.keys() {
                names.insert(key.clone());
            }
        }
    }
    names
}

fn resolve_virtual_paths_against_workspace(workspace: &Path, raw: Vec<String>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for raw_value in raw {
        // Skip empty/whitespace values. Without this, `virtual = ""` would
        // resolve to the workspace root and we'd misclassify any venv
        // directly under the workspace (e.g. `./.venv`) as Hatch-managed.
        let trimmed = raw_value.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Expand ~ and ${HOME}/${USERNAME} so configured values like
        // "~/.virtualenvs" resolve to the user home rather than being
        // joined onto the workspace as a relative path.
        let expanded = expand_path(PathBuf::from(trimmed));
        // If the home directory is unavailable, `expand_path()` returns
        // the input verbatim. Skip such entries rather than joining a
        // literal `~` onto the workspace root (e.g. `<workspace>/~/...`).
        if path_starts_with_tilde(&expanded) {
            continue;
        }
        let resolved = if expanded.is_absolute() {
            expanded
        } else {
            workspace.join(expanded)
        };
        dirs.push(norm_case(resolved));
    }
    dirs
}

/// Single entry point used by `configure()`: parses `pyproject.toml` and
/// `hatch.toml` ONCE each per workspace and derives both the resolved
/// virtual directories and the declared env names from the same parse.
fn resolve_workspace_hatch_config(workspace: &Path) -> (Vec<PathBuf>, HashSet<String>) {
    let sections = read_workspace_hatch_sections(workspace);
    let virtual_dirs =
        resolve_virtual_paths_against_workspace(workspace, extract_virtual_paths(&sections));
    let env_names = extract_env_names(&sections);
    (virtual_dirs, env_names)
}

/// Read the configured `dirs.env.virtual` paths for a workspace and resolve
/// each to an absolute directory. Both `pyproject.toml` (`[tool.hatch.dirs.env]`)
/// and a top-level `hatch.toml` (`[dirs.env]`) are checked.
///
/// Each value may be relative (resolved against the workspace root),
/// absolute, or use `~` / `${HOME}` expansion. Returns an empty Vec if the
/// workspace is not a Hatch project, or if no `virtual` value is configured.
///
/// The returned paths are cached regardless of whether they currently exist
/// on disk — a user may configure `virtual = ".hatch"` and create the env
/// later in this process lifetime, and we want subsequent `try_from()`
/// calls to recognise it without requiring the client to re-send `configure`.
/// `find_envs_in_flat_dir()` handles missing directories at discovery time.
#[cfg(test)]
fn resolve_project_virtual_dirs(workspace: &Path) -> Vec<PathBuf> {
    let sections = read_workspace_hatch_sections(workspace);
    resolve_virtual_paths_against_workspace(workspace, extract_virtual_paths(&sections))
}

/// Hatch's `default` environment is always implicitly available — Hatch
/// docs: "every project has a `default` environment". So even when
/// `[tool.hatch.envs.*]` declares no env, `default` is still a valid
/// env name. We include it in the allowlist unconditionally.
const HATCH_IMPLICIT_DEFAULT_ENV: &str = "default";

/// Read the set of Hatch env names declared for `workspace`. Reads
/// `[tool.hatch.envs.<name>]` from `pyproject.toml` and `[envs.<name>]`
/// from `hatch.toml`. The implicit `default` env is always included.
///
/// Used as a Hatch-specific guard so that venvs in a configured but
/// potentially shared `dirs.env.virtual` directory (e.g. `~/.virtualenvs`)
/// are only claimed when their leaf directory name matches a declared
/// env name — otherwise unrelated virtualenvwrapper / `venv` envs in
/// the same directory would be misclassified as Hatch.
#[cfg(test)]
fn resolve_project_env_names(workspace: &Path) -> HashSet<String> {
    let sections = read_workspace_hatch_sections(workspace);
    extract_env_names(&sections)
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

/// Pre-normalized allowlist of declared Hatch env names for a workspace,
/// used to filter venvs in a configured `dirs.env.virtual` directory.
///
/// Hatch's matrix feature creates per-variant directories named
/// `<env_name>.<variant>` (e.g. `test.py3.10`), so a leaf matches if it
/// equals a declared name *or* starts with `"<declared>."`. We precompute
/// both the normalized name and its `"<name>."` prefix so the hot path
/// (`try_from()` / `find_envs_in_flat_dir()`) avoids per-call `format!()`
/// allocations.
///
/// On case-insensitive filesystems (default on Windows) the on-disk leaf
/// may differ in case from the TOML key, so we lowercase both sides on
/// Windows at construction time. macOS volumes can be either case-sensitive
/// (default APFS) or case-insensitive (HFS+ / case-insensitive APFS), and
/// `norm_case()` itself does not case-fold on macOS — so we keep the
/// allowlist comparison byte-exact there to stay consistent with how paths
/// are normalized elsewhere in this crate.
#[derive(Clone, Default, Debug)]
struct EnvNameMatcher {
    /// (normalized_name, normalized_name + ".") pairs.
    entries: Vec<(String, String)>,
}

fn normalize_env_name(s: &str) -> String {
    #[cfg(windows)]
    {
        s.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        s.to_string()
    }
}

impl EnvNameMatcher {
    fn from_names<I: IntoIterator<Item = String>>(names: I) -> Self {
        let mut entries: Vec<(String, String)> = Vec::new();
        for raw in names {
            let n = normalize_env_name(&raw);
            if n.is_empty() {
                continue;
            }
            let prefix = format!("{n}.");
            entries.push((n, prefix));
        }
        Self { entries }
    }

    fn matches(&self, leaf: &str) -> bool {
        let leaf_n = normalize_env_name(leaf);
        self.entries
            .iter()
            .any(|(n, p)| leaf_n == *n || leaf_n.starts_with(p.as_str()))
    }
}

/// Walk `<dir>/<venv_name>/` and report each venv discovered. `matcher`
/// is the allow-list of leaf directory names that are considered Hatch
/// envs (so a shared dir like `~/.virtualenvs` only yields envs the
/// workspace actually declares).
fn find_envs_in_flat_dir(
    dir: &Path,
    project: Option<PathBuf>,
    matcher: &EnvNameMatcher,
) -> Vec<PythonEnvironment> {
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
        let leaf = match env_dir.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };
        if !matcher.matches(&leaf) {
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
    use std::sync::Mutex as StdMutex;
    use tempfile::TempDir;

    /// Serializes any test that mutates process-global environment variables
    /// (HOME / USERPROFILE / etc.) so cargo's default multi-threaded harness
    /// cannot race. Use `let _guard = ENV_LOCK.lock()...;` at the top of any
    /// test that reads or writes those variables.
    static ENV_LOCK: StdMutex<()> = StdMutex::new(());

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

    /// Canonicalize a temp path for test comparisons. On Windows, `TempDir`
    /// roots can come back as 8.3 short names (e.g. `C:\Users\RUNNER~1\...`)
    /// while paths surfaced via `fs::read_dir` or env-var expansion are in
    /// long form (`C:\Users\runneradmin\...`). Without this both sides of
    /// `PathBuf` equality checks would not match on CI runners. The
    /// `\\?\` verbatim prefix added by `fs::canonicalize` is stripped so the
    /// resulting path matches what production code produces.
    fn canonicalize_for_test(p: &Path) -> PathBuf {
        let canon = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        #[cfg(windows)]
        {
            let s = canon.to_string_lossy().to_string();
            if let Some(stripped) = s.strip_prefix(r"\\?\") {
                return PathBuf::from(stripped);
            }
        }
        canon
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
            workspace_virtual_dirs: Arc::new(Mutex::new(vec![])),
        }
    }

    /// Build a locator with a single configured workspace whose virtual dirs
    /// have been resolved up-front (mirrors what `configure()` does).
    fn make_locator_with_workspace(
        default_virtual_dir: Option<PathBuf>,
        workspace: &Path,
    ) -> Hatch {
        let virtual_dirs = resolve_project_virtual_dirs(workspace);
        let env_names = resolve_project_env_names(workspace);
        Hatch {
            default_virtual_dir,
            workspace_virtual_dirs: Arc::new(Mutex::new(vec![Arc::new(WorkspaceEntry {
                workspace: workspace.to_path_buf(),
                virtual_dirs,
                matcher: EnvNameMatcher::from_names(env_names),
            })])),
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

        let locator = make_locator_with_workspace(None, &project);
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

        let locator = make_locator_with_workspace(None, &project);
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

        let locator = make_locator_with_workspace(None, &project);
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
        let matcher = EnvNameMatcher::from_names(resolve_project_env_names(&project));
        let envs = find_envs_in_flat_dir(&virtual_dirs[0], Some(project.clone()), &matcher);
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

    #[test]
    fn resolve_project_virtual_dirs_expands_tilde() {
        // A configured value of "~/.virtualenvs" must resolve against the
        // user's home directory, not be joined onto the workspace as a
        // relative path. We fake $HOME / %USERPROFILE% to point at a
        // tempdir we control, then make sure the expanded path is what we
        // get back.
        //
        // `expand_path()` reads HOME / USERPROFILE from the *process* env, so
        // this test mutates global state. We serialize against any other
        // env-mutating test in this crate via `ENV_LOCK` so cargo's default
        // multi-threaded harness cannot race.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        let temp = TempDir::new().unwrap();
        let fake_home = temp.path().join("home");
        let virtualenvs = fake_home.join(".virtualenvs");
        fs::create_dir_all(&virtualenvs).unwrap();
        let project = temp.path().join("proj");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("pyproject.toml"),
            b"[tool.hatch.dirs.env]\nvirtual = \"~/.virtualenvs\"\n",
        )
        .unwrap();

        let prev_home = std::env::var_os("HOME");
        let prev_user_profile = std::env::var_os("USERPROFILE");
        std::env::set_var("HOME", &fake_home);
        std::env::set_var("USERPROFILE", &fake_home);

        let dirs = resolve_project_virtual_dirs(&project);

        // Restore env regardless of assertion outcome.
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match prev_user_profile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }

        assert_eq!(dirs, vec![norm_case(&virtualenvs)]);
    }

    #[test]
    fn resolve_project_virtual_dirs_skips_unexpanded_tilde() {
        // If HOME / USERPROFILE are unset, `expand_path("~/.virtualenvs")`
        // returns the input verbatim. We must not join `~` onto the
        // workspace root (yielding `<workspace>/~/.virtualenvs`) or pass
        // a tilde-prefixed path through `norm_case()` — both would
        // misclassify unrelated envs.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        let temp = TempDir::new().unwrap();
        let project = temp.path().join("proj");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("pyproject.toml"),
            b"[tool.hatch.dirs.env]\nvirtual = \"~/.virtualenvs\"\n",
        )
        .unwrap();

        let prev_home = std::env::var_os("HOME");
        let prev_user_profile = std::env::var_os("USERPROFILE");
        std::env::remove_var("HOME");
        std::env::remove_var("USERPROFILE");

        let dirs = resolve_project_virtual_dirs(&project);

        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match prev_user_profile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }

        assert!(
            dirs.is_empty(),
            "unexpanded tilde paths must not be claimed: got {dirs:?}"
        );
    }

    #[test]
    fn configure_caches_workspace_virtual_dirs() {
        // try_from() must not re-read pyproject.toml on every call; configure()
        // is responsible for resolving and caching the virtual dirs once.
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("pyproject.toml"),
            b"[tool.hatch.dirs.env]\nvirtual = \".hatch\"\n",
        )
        .unwrap();
        let virtual_dir = project.join(".hatch");
        fs::create_dir_all(&virtual_dir).unwrap();

        let locator = make_locator(None);
        let config = Configuration {
            workspace_directories: Some(vec![project.clone()]),
            ..Configuration::default()
        };
        locator.configure(&config);

        let cached = locator.workspace_virtual_dirs.lock().unwrap().clone();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].workspace, project);
        assert_eq!(cached[0].virtual_dirs, vec![norm_case(&virtual_dir)]);
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
        // redirected Hatch elsewhere. The path itself does not need to
        // exist at construction time (it may be created later in the
        // process lifetime); we only require that the returned value
        // points at HATCH_DATA_DIR/env/virtual, not the platform default.
        let temp = TempDir::new().unwrap();
        let custom = temp.path().join("does-not-exist-yet");
        let mut vars = HashMap::new();
        vars.insert(
            "HATCH_DATA_DIR".to_string(),
            custom.to_string_lossy().to_string(),
        );
        let env = TestEnv {
            home: Some(temp.path().to_path_buf()),
            vars,
        };
        let expected = norm_case(custom.join("env").join("virtual"));
        assert_eq!(get_default_virtual_dir(&env), Some(expected));
    }

    #[test]
    fn default_virtual_dir_expands_tilde_in_hatch_data_dir() {
        // A value like `HATCH_DATA_DIR=~/.local/share/hatch` must be
        // expanded against the user's home rather than be treated as a
        // literal `~` directory.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        let temp = TempDir::new().unwrap();
        let fake_home = temp.path().join("home");
        fs::create_dir_all(&fake_home).unwrap();
        // Canonicalize so 8.3 short names on Windows CI runners don't
        // cause spurious path mismatches when comparing against the
        // value produced by `expand_path` + `norm_case`.
        let fake_home = canonicalize_for_test(&fake_home);

        let prev_home = std::env::var_os("HOME");
        let prev_user_profile = std::env::var_os("USERPROFILE");
        std::env::set_var("HOME", &fake_home);
        std::env::set_var("USERPROFILE", &fake_home);

        let mut vars = HashMap::new();
        vars.insert(
            "HATCH_DATA_DIR".to_string(),
            "~/.local/share/hatch".to_string(),
        );
        let env = TestEnv {
            home: Some(fake_home.clone()),
            vars,
        };
        let resolved = get_default_virtual_dir(&env);

        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match prev_user_profile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }

        // Compare via path components rather than byte-exact strings: on
        // Windows, `expand_path` may preserve the forward-slash separators
        // present in the input value (`~/.local/share/hatch`) while
        // `PathBuf::join` adds backslashes, leading to a mixed-separator
        // representation that still refers to the same logical path. Path
        // component iteration is separator-agnostic.
        let resolved = resolved.expect("HATCH_DATA_DIR resolution returned None");
        let expected = fake_home
            .join(".local")
            .join("share")
            .join("hatch")
            .join("env")
            .join("virtual");
        let expected_components: Vec<_> = expected.components().collect();
        let resolved_components: Vec<_> = resolved.components().collect();
        assert_eq!(resolved_components, expected_components);
    }

    #[test]
    fn default_virtual_dir_treats_whitespace_hatch_data_dir_as_unset() {
        // Whitespace-only HATCH_DATA_DIR must be treated as unset so we
        // fall back to the platform default rather than resolving to
        // a literal whitespace directory.
        let temp = TempDir::new().unwrap();
        let mut vars = HashMap::new();
        vars.insert("HATCH_DATA_DIR".to_string(), "   ".to_string());
        let env = TestEnv {
            home: Some(temp.path().to_path_buf()),
            vars,
        };
        // Should NOT be the literal "   /env/virtual"; should resolve via
        // the platform default (or None if home is unavailable).
        let resolved = get_default_virtual_dir(&env);
        if let Some(p) = resolved {
            assert!(!p.to_string_lossy().contains("   "));
        }
    }

    #[test]
    fn resolve_project_virtual_dirs_skips_empty_value() {
        // `virtual = ""` must not resolve to the workspace root and
        // misclassify unrelated venvs under the workspace as Hatch.
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("proj");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("pyproject.toml"),
            b"[tool.hatch.dirs.env]\nvirtual = \"\"\n",
        )
        .unwrap();
        assert!(resolve_project_virtual_dirs(&project).is_empty());
    }

    #[test]
    fn resolve_project_virtual_dirs_skips_whitespace_value() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("proj");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("pyproject.toml"),
            b"[tool.hatch.dirs.env]\nvirtual = \"   \"\n",
        )
        .unwrap();
        assert!(resolve_project_virtual_dirs(&project).is_empty());
    }

    #[test]
    fn resolve_project_env_names_includes_implicit_default() {
        // Hatch always provides a `default` env, even if `[tool.hatch.envs.*]`
        // declares none.
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("proj");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("pyproject.toml"),
            b"[tool.hatch.dirs.env]\nvirtual = \".hatch\"\n",
        )
        .unwrap();
        let names = resolve_project_env_names(&project);
        assert!(names.contains("default"));
    }

    #[test]
    fn resolve_project_env_names_reads_declared_envs() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("proj");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("pyproject.toml"),
            b"[tool.hatch.envs.default]\n[tool.hatch.envs.test]\n[tool.hatch.envs.docs]\n",
        )
        .unwrap();
        let names = resolve_project_env_names(&project);
        assert!(names.contains("default"));
        assert!(names.contains("test"));
        assert!(names.contains("docs"));
    }

    #[test]
    fn find_envs_in_flat_dir_filters_non_declared_envs() {
        // A shared `dirs.env.virtual` directory (e.g. ~/.virtualenvs) may
        // contain envs created by other tools. Only envs whose leaf
        // directory name matches a declared Hatch env should be claimed.
        let temp = TempDir::new().unwrap();
        let shared = temp.path().join("shared");
        fs::create_dir_all(&shared).unwrap();
        // Canonicalize so 8.3 short names on Windows CI runners don't
        // cause spurious path mismatches when comparing prefixes that
        // were surfaced via `fs::read_dir`.
        let shared = canonicalize_for_test(&shared);

        // Hatch-managed env.
        let hatch_env = shared.join("default");
        write_pyvenv_cfg(&hatch_env, "default", "3.11.0");
        write_python_exe(&hatch_env);

        // Unrelated env (e.g. virtualenvwrapper) in the same dir.
        let foreign = shared.join("some-other-project");
        write_pyvenv_cfg(&foreign, "some-other-project", "3.11.0");
        write_python_exe(&foreign);

        let mut raw = HashSet::new();
        raw.insert("default".to_string());
        let matcher = EnvNameMatcher::from_names(raw);
        let envs = find_envs_in_flat_dir(&shared, None, &matcher);
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].prefix, Some(hatch_env));
    }

    #[test]
    fn find_envs_in_flat_dir_accepts_matrix_variants() {
        // Hatch matrix envs land on disk as `<env>.<variant>` (e.g.
        // `test.py3.10`). They must still be claimed by the declared env
        // `test`.
        let temp = TempDir::new().unwrap();
        let shared = temp.path().join("shared");
        fs::create_dir_all(&shared).unwrap();

        let v1 = shared.join("test.py3.10");
        write_pyvenv_cfg(&v1, "test.py3.10", "3.10.0");
        write_python_exe(&v1);
        let v2 = shared.join("test.py3.11");
        write_pyvenv_cfg(&v2, "test.py3.11", "3.11.0");
        write_python_exe(&v2);
        // Foreign env must still be rejected.
        let foreign = shared.join("unrelated");
        write_pyvenv_cfg(&foreign, "unrelated", "3.11.0");
        write_python_exe(&foreign);

        let mut raw = HashSet::new();
        raw.insert("test".to_string());
        let matcher = EnvNameMatcher::from_names(raw);
        let envs = find_envs_in_flat_dir(&shared, None, &matcher);
        assert_eq!(envs.len(), 2);
    }

    #[cfg(windows)]
    #[test]
    fn env_name_matches_is_case_insensitive_on_windows() {
        let mut names = HashSet::new();
        names.insert("Default".to_string());
        let matcher = EnvNameMatcher::from_names(names);
        assert!(matcher.matches("default"));
        assert!(matcher.matches("DEFAULT"));
    }

    #[test]
    fn try_from_rejects_unknown_leaf_under_configured_virtual_dir() {
        // Workspace declares only `default`. A sibling venv created by
        // another tool in the same configured `virtual` directory must
        // not be claimed.
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("proj");
        fs::create_dir_all(&project).unwrap();
        let shared = temp.path().join("shared");
        fs::create_dir_all(&shared).unwrap();
        fs::write(
            project.join("pyproject.toml"),
            format!(
                "[tool.hatch.dirs.env]\nvirtual = \"{}\"\n[tool.hatch.envs.default]\n",
                shared.display().to_string().replace('\\', "\\\\")
            ),
        )
        .unwrap();

        let foreign = shared.join("some-other-project");
        write_pyvenv_cfg(&foreign, "some-other-project", "3.11.0");
        let exe = write_python_exe(&foreign);

        let locator = make_locator_with_workspace(None, &project);
        let env = PythonEnv::new(exe, Some(foreign), None);
        assert!(
            locator.try_from(&env).is_none(),
            "Hatch should not claim non-declared envs in a shared virtual dir"
        );
    }
}
