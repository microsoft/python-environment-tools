---
name: "rust-locator-patterns"
description: "PET (Python Environment Tools) Rust coding conventions: locator ordering, thread safety, platform gating, JSONRPC rules, version detection, and path/symlink handling. Reference when writing or reviewing Rust code in this repo to avoid convention violations that cause review churn."
---

# PET Rust Locator Patterns & Conventions

Domain knowledge for writing correct Rust code in the Python Environment Tools (PET) repository. Following these patterns prevents the most common review feedback and CI failures.

## Locator Ordering (CRITICAL)

The order of locators in `create_locators()` in `crates/pet/src/locators.rs` determines identification priority. **More specific locators MUST come before generic ones.**

### Priority Chain (required order):
1. **Windows-specific:** Windows Store → Windows Registry → WinPython
2. **Managed environments:** PyEnv → Pixi → Conda
3. **Virtual envs (specific → generic):** Poetry → PipEnv → VirtualEnvWrapper → Venv → VirtualEnv
4. **macOS-specific:** Homebrew → MacXCode → MacCmdLineTools → MacPythonOrg
5. **Linux fallback:** LinuxGlobalPython (MUST BE LAST)

**Why it matters:** If `Venv` runs before `Poetry`, a Poetry environment (which IS a venv) gets misidentified as a plain venv. The first locator whose `try_from()` returns `Some` wins.

## Environment Identification (`try_from`)

Each `try_from()` must be **precise enough to avoid false positives**:

- **Poetry:** Match `{project-name}-{8-char-hash}-py{major}.{minor}` naming pattern AND verify location is in poetry cache dir
- **Pipenv:** Look for `.project` file in centralized dir with hash-based naming
- **Conda:** Require `conda-meta/` directory with history file
- **Pixi:** Require `.pixi/envs/` in path structure
- **venv:** Check for `pyvenv.cfg` file (but many env types have this — venv is a fallback)
- **VirtualEnv:** Activation scripts WITHOUT `pyvenv.cfg` (most generic, last resort)

**Red flag:** A `try_from()` that only checks `pyvenv.cfg` existence — that matches ALL venvs, not just one type.

## JSONRPC Protocol Rules

The server communicates via stdin/stdout. **Any stdout pollution breaks the protocol.**

- **NEVER use `println!`** — it writes to stdout and corrupts JSONRPC messages
- Use `log` crate macros: `info!()`, `trace!()`, `warn!()`, `error!()`
- Log with elapsed time for diagnostic handlers (established pattern)
- All tracing/logging writes to stderr via subscriber configuration

## Thread Safety Patterns

PET uses heavy concurrency for parallel discovery:

- Shared state requires `Arc<Mutex<T>>` or `Arc<RwLock<T>>`
- Keep lock scopes minimal — clone data out, drop lock, then process
- Use `.expect("context")` on mutex locks, not bare `.unwrap()`
- No nested locks (deadlock risk)
- Consider `thread::scope` for structured concurrency

## Platform-Specific Code

Use `#[cfg(...)]` attributes for conditional compilation:

```rust
#[cfg(windows)]
use pet_windows_registry::WindowsRegistry;

#[cfg(unix)]
fn resolve_path(p: &Path) -> PathBuf { std::fs::canonicalize(p).unwrap_or(p.to_path_buf()) }

#[cfg(target_os = "macos")]
use pet_homebrew::Homebrew;
```

**Do NOT use `if cfg!(windows) { ... }` for code that imports platform-specific modules** — the code inside still gets compiled on all platforms.

### Platform Gotchas:
- **Windows:** Avoid `canonicalize()` for directory junctions (breaks Scoop symlinks). Use `norm_case()` instead.
- **macOS:** Homebrew has complex symlink chains; `resolve_symlink` before `canonicalize`
- **Linux:** `/bin` may symlink to `/usr/bin` — handle both

## Version Detection (Prefer File-Based)

**Order of preference (cheapest first):**
1. `pyvenv.cfg` — `version` or `version_info` field
2. `conda-meta/python-*.json` — package metadata
3. `Include/patchlevel.h` — CPython source header
4. Parse from executable path (e.g., `python3.11`)
5. Spawn Python `--version` — **LAST RESORT** (expensive, defeats PET's purpose)

## Path & Symlink Handling

- Use `pet_fs::path::resolve_symlink()` — the project's own resolver
- For comparison: `canonicalize().unwrap_or(original)` then `norm_case()` for Windows UNC prefix handling
- Preserve original user-facing paths; resolve only for identification

## Testing Conventions

- Feature-gated CI tests: `#[cfg_attr(feature = "ci", test)]` with `#[allow(dead_code)]`
- Feature flags: `ci`, `ci-poetry-global`, `ci-jupyter-container`, `ci-homebrew-container`
- Verification pattern: spawn Python, use `try_from()`, test symlinks, use `resolve`
- Use `cargo test --features ci` for CI-gated tests

## Serialization

- All JSON-serialized structs use `#[serde(rename_all = "camelCase")]`
- Matches the JSONRPC API conventions consumed by VS Code Python extension

## Pre-Commit Requirements

Always run before committing:
```bash
cargo fmt --all
cargo clippy --all -- -D warnings
```
Never suppress clippy warnings with `#[allow(...)]` without justification.

## Conda-Specific Patterns

- Conda manager detection takes precedence over mamba: `get_conda_manager(path).or_else(|| get_mamba_manager(path))`
- Managers reported separately (Conda, Mamba) but environments remain `PythonEnvironmentKind::Conda`
- Support detection from history files and conda-meta directories
