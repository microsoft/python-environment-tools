# Python Environment Tools (PET) - AI Coding Agent Instructions

## Project Overview

This is a high-performance Rust-based tool for discovering Python environments and virtual environments. It operates as a JSONRPC server consumed by the VS Code Python extension to avoid spawning Python processes repeatedly.

## Architecture

### Core Concepts

- **Locators**: Modular environment discovery components implementing the `Locator` trait (`crates/pet-core/src/lib.rs`)
- **JSONRPC Server**: Main communication interface (`crates/pet/src/jsonrpc.rs`) with stdio/stdout protocol
- **Environment Types**: 15+ supported Python installations (Conda, Poetry, PyEnv, Homebrew, Windows Store, etc.)
- **Reporter Pattern**: Asynchronous environment discovery reporting via the `Reporter` trait

### Key Architecture Files

- `crates/pet/src/locators.rs` - Ordered locator creation and fallback identification logic
- `crates/pet/src/find.rs` - Multi-threaded environment discovery coordination
- `crates/pet-core/src/lib.rs` - Core traits and configuration structures
- `docs/JSONRPC.md` - Complete API specification with TypeScript interfaces

## Development Workflow

### Building & Testing

```bash
# Standard build
cargo build

# Release build (optimized for performance)
cargo build --release

# Run tests with specific CI features
cargo test --features ci
cargo test --features ci-poetry-global

# Run JSONRPC server
./target/debug/pet server
```

### Feature-Gated Testing

Tests use feature flags for different environments:

- `ci` - General CI environment tests
- `ci-jupyter-container` - Jupyter container-specific tests
- `ci-homebrew-container` - Homebrew container tests
- `ci-poetry-*` - Poetry-specific test variants

### Locator Development Pattern

When adding new environment types:

1. **Create new crate**: `crates/pet-{name}/`
2. **Implement Locator trait**: Key methods are `try_from()` (identification) and `find()` (discovery)
3. **Add to locator chain**: Update `create_locators()` in `crates/pet/src/locators.rs` - ORDER MATTERS
4. **Platform-specific**: Use `#[cfg(windows)]`, `#[cfg(unix)]`, `#[cfg(target_os = "macos")]`

Example structure:

```rust
impl Locator for MyLocator {
    fn get_kind(&self) -> LocatorKind { LocatorKind::MyType }
    fn supported_categories(&self) -> Vec<PythonEnvironmentKind> { vec![PythonEnvironmentKind::MyType] }
    fn try_from(&self, env: &PythonEnv) -> Option<PythonEnvironment> { /* identification logic */ }
    fn find(&self, reporter: &dyn Reporter) { /* discovery logic */ }
}
```

## Critical Patterns

### Performance Principles (from `crates/pet/README.md`)

1. **Avoid spawning processes** - Extract info from files/filesystem when possible
2. **Report immediately** - Use Reporter pattern for async discovery
3. **Complete information** - Gather all environment details in one pass, not incrementally

### JSONRPC Communication Flow

1. Client sends `configure` request (must be first)
2. Client sends `refresh` request to discover environments
3. Server sends `environment` notifications as discoveries happen
4. Optional: `resolve` request for individual Python executables

### Testing Verification Pattern

Tests validate discovered environments using 4 verification methods:

1. Spawn Python to verify `sys.prefix` and `sys.version`
2. Use `try_from()` with executable to get same info
3. Test symlink identification
4. Use `resolve` method for consistency

## Environment-Specific Notes

### Conda Environments

- Supports detection from history files and conda-meta directories
- Manager detection via spawning conda executable in background threads
- Complex prefix/name relationships for base vs named environments

### Poetry Environments

- Hash-based environment naming: `{project-name}-{hash}-py`
- Project-specific virtual environments in configured cache directories
- Configuration hierarchy: local poetry.toml → global config

### Platform Differences

- **Windows**: Registry + Windows Store detection, different path separators
- **macOS**: Xcode Command Line Tools, python.org, Homebrew paths
- **Linux**: Global system paths (`/usr/bin`, `/usr/local/bin`)

## Common Gotchas

- **Locator order matters** in `create_locators()` - more specific before generic
- **Thread safety** - Heavy use of Arc/Mutex for concurrent discovery
- **Feature flags** - Many tests only run with specific CI features enabled
- **Path canonicalization** - Symlink resolution varies by platform
- **Caching** - Optional cache directory for expensive operations (conda spawning)

## Files to Read First

1. `docs/JSONRPC.md` - Understanding the external API
2. `crates/pet/src/locators.rs` - Core architecture patterns
3. `crates/pet-core/src/lib.rs` - Essential traits and types
4. `crates/pet/tests/ci_test.rs` - Comprehensive testing patterns


## Scripts
- Use `cargo fetch` to download all dependencies
- Use `rustup component add clippy` to install Clippy linter
- Use `cargo fmt --all` to format code in all packages
- Use `cargo clippy --all-features -- -Dwarnings` to check for linter issues
- Use `cargo clippy --all-features --fix --allow-dirty` to automatically fix linter issues
- Use `cargo build` to build the project
- Use `cargo test --all` to test all packages (this can take a few seconds)
- Use `cargo test [TESTNAME]` to test a specific test
- Use `cargo test -p [SPEC]` to test a specific package
- Use `cargo test --all` to test all packages
