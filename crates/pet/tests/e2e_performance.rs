// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! End-to-end performance tests for the pet JSONRPC server.
//!
//! These tests spawn the pet server as a subprocess and communicate via JSONRPC
//! to measure discovery performance from a client perspective.

use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod common;

/// JSONRPC request ID counter
static REQUEST_ID: AtomicU32 = AtomicU32::new(1);

/// Performance metrics collected during tests
#[derive(Debug, Clone, Default)]
pub struct PerformanceMetrics {
    /// Time to spawn server and get first response (configure)
    pub server_startup_ms: u128,
    /// Time for full machine refresh
    pub full_refresh_ms: u128,
    /// Time for workspace-scoped refresh
    pub workspace_refresh_ms: Option<u128>,
    /// Time for kind-specific refresh
    pub kind_refresh_ms: HashMap<String, u128>,
    /// Number of environments discovered
    pub environments_count: usize,
    /// Number of managers discovered
    pub managers_count: usize,
    /// Time to first environment notification
    pub time_to_first_env_ms: Option<u128>,
    /// Resolve times (cold and warm)
    pub resolve_times_ms: Vec<u128>,
}

/// Refresh result from server
#[derive(Debug, Clone, Deserialize)]
pub struct RefreshResult {
    pub duration: u128,
}

/// Environment notification from server
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub executable: Option<String>,
    pub kind: Option<String>,
    #[allow(dead_code)]
    pub version: Option<String>,
}

/// Manager notification from server
#[derive(Debug, Clone, Deserialize)]
pub struct Manager {
    #[allow(dead_code)]
    pub tool: Option<String>,
    #[allow(dead_code)]
    pub executable: Option<String>,
}

/// Shared state for handling notifications
struct SharedState {
    environments: Mutex<Vec<Environment>>,
    managers: Mutex<Vec<Manager>>,
    first_env_time: Mutex<Option<Instant>>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            environments: Mutex::new(Vec::new()),
            managers: Mutex::new(Vec::new()),
            first_env_time: Mutex::new(None),
        }
    }

    fn handle_notification(&self, method: &str, params: Value) {
        match method {
            "environment" => {
                // Record time to first environment
                {
                    let mut first_env = self.first_env_time.lock().unwrap();
                    if first_env.is_none() {
                        *first_env = Some(Instant::now());
                    }
                }

                if let Ok(env) = serde_json::from_value::<Environment>(params) {
                    self.environments.lock().unwrap().push(env);
                }
            }
            "manager" => {
                if let Ok(mgr) = serde_json::from_value::<Manager>(params) {
                    self.managers.lock().unwrap().push(mgr);
                }
            }
            "log" | "telemetry" => {
                // Ignore log and telemetry notifications
            }
            _ => {
                // Unknown notification
            }
        }
    }

    fn clear(&self) {
        self.environments.lock().unwrap().clear();
        self.managers.lock().unwrap().clear();
        *self.first_env_time.lock().unwrap() = None;
    }
}

/// JSONRPC client for communicating with the pet server
pub struct PetClient {
    process: Child,
    state: Arc<SharedState>,
    start_time: Instant,
}

impl PetClient {
    /// Spawn the pet server and create a client
    pub fn spawn() -> Result<Self, String> {
        let pet_exe = get_pet_executable();

        if !pet_exe.exists() {
            return Err(format!(
                "pet executable not found at {:?}. Run `cargo build --release` first.",
                pet_exe
            ));
        }

        let start_time = Instant::now();

        let process = Command::new(&pet_exe)
            .arg("server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn pet server: {}", e))?;

        Ok(Self {
            process,
            state: Arc::new(SharedState::new()),
            start_time,
        })
    }

    /// Send a JSONRPC request and wait for response
    fn send_request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = REQUEST_ID.fetch_add(1, Ordering::SeqCst);
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        let request_str = serde_json::to_string(&request)
            .map_err(|e| format!("Failed to serialize request: {}", e))?;

        let content_length = request_str.len();
        let message = format!("Content-Length: {}\r\n\r\n{}", content_length, request_str);

        // Write request
        {
            let stdin = self.process.stdin.as_mut().ok_or("Failed to get stdin")?;
            stdin
                .write_all(message.as_bytes())
                .map_err(|e| format!("Failed to write request: {}", e))?;
            stdin
                .flush()
                .map_err(|e| format!("Failed to flush stdin: {}", e))?;
        }

        // Clone state reference for use in the loop
        let state = self.state.clone();

        // Read response - handle notifications until we get our response
        let stdout = self.process.stdout.as_mut().ok_or("Failed to get stdout")?;
        let mut reader = BufReader::new(stdout);

        loop {
            // Read headers until empty line
            let mut content_length: Option<usize> = None;
            loop {
                let mut header_line = String::new();
                reader
                    .read_line(&mut header_line)
                    .map_err(|e| format!("Failed to read header: {}", e))?;

                let trimmed = header_line.trim();
                if trimmed.is_empty() {
                    // End of headers
                    break;
                }

                if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
                    content_length = Some(
                        len_str
                            .parse()
                            .map_err(|e| format!("Failed to parse content length: {}", e))?,
                    );
                }
                // Ignore Content-Type and other headers
            }

            let content_length = content_length.ok_or("Missing Content-Length header")?;

            // Read body
            let mut body = vec![0u8; content_length];
            reader
                .read_exact(&mut body)
                .map_err(|e| format!("Failed to read body: {}", e))?;

            let body_str = String::from_utf8_lossy(&body);
            let value: Value = serde_json::from_str(&body_str)
                .map_err(|e| format!("Failed to parse response: {}", e))?;

            // Check if this is a notification or our response
            if let Some(notif_method) = value.get("method").and_then(|m| m.as_str()) {
                // Handle notifications using the cloned state reference
                state.handle_notification(
                    notif_method,
                    value.get("params").cloned().unwrap_or(Value::Null),
                );
                continue;
            }

            // Check if this is our response
            if let Some(response_id) = value.get("id").and_then(|i| i.as_u64()) {
                if response_id as u32 == id {
                    if let Some(error) = value.get("error") {
                        return Err(format!("JSONRPC error: {:?}", error));
                    }
                    return Ok(value.get("result").cloned().unwrap_or(Value::Null));
                }
            }
        }
    }

    /// Configure the server
    pub fn configure(&mut self, config: Value) -> Result<Duration, String> {
        let start = Instant::now();
        self.send_request("configure", config)?;
        Ok(start.elapsed())
    }

    /// Refresh environments
    pub fn refresh(&mut self, params: Option<Value>) -> Result<(RefreshResult, Duration), String> {
        // Clear previous results
        self.state.clear();

        let start = Instant::now();
        let result = self.send_request("refresh", params.unwrap_or(json!({})))?;
        let elapsed = start.elapsed();

        let refresh_result: RefreshResult = serde_json::from_value(result)
            .map_err(|e| format!("Failed to parse refresh result: {}", e))?;

        Ok((refresh_result, elapsed))
    }

    /// Resolve a Python executable
    pub fn resolve(&mut self, executable: &str) -> Result<(Value, Duration), String> {
        let start = Instant::now();
        let result = self.send_request("resolve", json!({ "executable": executable }))?;
        Ok((result, start.elapsed()))
    }

    /// Get collected environments
    pub fn get_environments(&self) -> Vec<Environment> {
        self.state.environments.lock().unwrap().clone()
    }

    /// Get collected managers
    pub fn get_managers(&self) -> Vec<Manager> {
        self.state.managers.lock().unwrap().clone()
    }

    /// Get time from start to first environment
    pub fn time_to_first_env(&self) -> Option<Duration> {
        self.state
            .first_env_time
            .lock()
            .unwrap()
            .map(|t| t.duration_since(self.start_time))
    }

    /// Get startup time
    #[allow(dead_code)]
    pub fn startup_time(&self) -> Duration {
        self.start_time.elapsed()
    }
}

impl Drop for PetClient {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// Get the path to the pet executable
fn get_pet_executable() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target");

    // Prefer release build for performance tests
    let release_exe = if cfg!(windows) {
        target_dir.join("release").join("pet.exe")
    } else {
        target_dir.join("release").join("pet")
    };

    if release_exe.exists() {
        return release_exe;
    }

    // Fall back to debug build
    if cfg!(windows) {
        target_dir.join("debug").join("pet.exe")
    } else {
        target_dir.join("debug").join("pet")
    }
}

/// Get a temporary cache directory for tests
fn get_test_cache_dir() -> PathBuf {
    let tmp = env::temp_dir();
    tmp.join("pet-e2e-perf-tests")
        .join(format!("cache-{}", std::process::id()))
}

/// Get workspace directory (current project root)
fn get_workspace_dir() -> PathBuf {
    env::var("GITHUB_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf()
        })
}

// ============================================================================
// Performance Tests
// ============================================================================

#[cfg_attr(feature = "ci-perf", test)]
#[allow(dead_code)]
fn test_server_startup_performance() {
    let start = Instant::now();
    let mut client = PetClient::spawn().expect("Failed to spawn server");
    let spawn_time = start.elapsed();

    let cache_dir = get_test_cache_dir();
    let workspace_dir = get_workspace_dir();

    let config = json!({
        "workspaceDirectories": [workspace_dir],
        "cacheDirectory": cache_dir
    });

    let configure_time = client.configure(config).expect("Failed to configure");

    println!("=== Server Startup Performance ===");
    println!("Server spawn time: {:?}", spawn_time);
    println!("Configure request time: {:?}", configure_time);
    println!("Total startup time: {:?}", spawn_time + configure_time);

    // Assert reasonable startup time (should be under 1 second on most machines)
    assert!(
        spawn_time.as_millis() < 5000,
        "Server spawn took too long: {:?}",
        spawn_time
    );
    assert!(
        configure_time.as_millis() < 1000,
        "Configure took too long: {:?}",
        configure_time
    );
}

#[cfg_attr(feature = "ci-perf", test)]
#[allow(dead_code)]
fn test_full_refresh_performance() {
    let mut client = PetClient::spawn().expect("Failed to spawn server");

    let cache_dir = get_test_cache_dir();
    let workspace_dir = get_workspace_dir();

    let config = json!({
        "workspaceDirectories": [workspace_dir],
        "cacheDirectory": cache_dir
    });

    client.configure(config).expect("Failed to configure");

    // Full machine refresh
    let (result, client_elapsed) = client.refresh(None).expect("Failed to refresh");
    let environments = client.get_environments();
    let managers = client.get_managers();

    println!("=== Full Refresh Performance ===");
    println!("Server-reported duration: {}ms", result.duration);
    println!("Client-measured duration: {:?}", client_elapsed);
    println!("Environments discovered: {}", environments.len());
    println!("Managers discovered: {}", managers.len());

    if let Some(time_to_first) = client.time_to_first_env() {
        println!("Time to first environment: {:?}", time_to_first);
    }

    // Log environment kinds found
    let mut kind_counts: HashMap<String, usize> = HashMap::new();
    for env in &environments {
        if let Some(kind) = &env.kind {
            *kind_counts.entry(kind.clone()).or_insert(0) += 1;
        }
    }
    println!("Environment kinds: {:?}", kind_counts);

    // Assert we found at least some environments (CI should always have Python installed)
    assert!(
        !environments.is_empty(),
        "No environments discovered - this is unexpected"
    );
}

#[cfg_attr(feature = "ci-perf", test)]
#[allow(dead_code)]
fn test_workspace_scoped_refresh_performance() {
    let mut client = PetClient::spawn().expect("Failed to spawn server");

    let cache_dir = get_test_cache_dir();
    let workspace_dir = get_workspace_dir();

    let config = json!({
        "workspaceDirectories": [workspace_dir.clone()],
        "cacheDirectory": cache_dir
    });

    client.configure(config).expect("Failed to configure");

    // Workspace-scoped refresh
    let (result, client_elapsed) = client
        .refresh(Some(json!({ "searchPaths": [workspace_dir] })))
        .expect("Failed to refresh");

    let environments = client.get_environments();

    println!("=== Workspace-Scoped Refresh Performance ===");
    println!("Server-reported duration: {}ms", result.duration);
    println!("Client-measured duration: {:?}", client_elapsed);
    println!("Environments discovered: {}", environments.len());

    // Workspace-scoped should be faster than full refresh
    // (though we don't assert this as it depends on the environment)
}

#[cfg_attr(feature = "ci-perf", test)]
#[allow(dead_code)]
fn test_kind_specific_refresh_performance() {
    let mut client = PetClient::spawn().expect("Failed to spawn server");

    let cache_dir = get_test_cache_dir();
    let workspace_dir = get_workspace_dir();

    let config = json!({
        "workspaceDirectories": [workspace_dir],
        "cacheDirectory": cache_dir
    });

    client.configure(config).expect("Failed to configure");

    // Test different environment kinds
    let kinds = ["Conda", "Venv", "VirtualEnv", "Pyenv"];

    println!("=== Kind-Specific Refresh Performance ===");

    for kind in kinds {
        let (result, client_elapsed) = client
            .refresh(Some(json!({ "searchKind": kind })))
            .expect(&format!("Failed to refresh for kind {}", kind));

        let environments = client.get_environments();

        println!(
            "{}: {}ms (server), {:?} (client), {} envs",
            kind,
            result.duration,
            client_elapsed,
            environments.len()
        );
    }
}

#[cfg_attr(feature = "ci-perf", test)]
#[allow(dead_code)]
fn test_resolve_performance() {
    let mut client = PetClient::spawn().expect("Failed to spawn server");

    let cache_dir = get_test_cache_dir();
    let workspace_dir = get_workspace_dir();

    let config = json!({
        "workspaceDirectories": [workspace_dir],
        "cacheDirectory": cache_dir
    });

    client.configure(config).expect("Failed to configure");

    // First, discover environments
    client.refresh(None).expect("Failed to refresh");
    let environments = client.get_environments();

    if environments.is_empty() {
        println!("No environments found to test resolve performance");
        return;
    }

    println!("=== Resolve Performance ===");

    // Find an environment with an executable to resolve
    let env_with_exe = environments.iter().find(|e| e.executable.is_some());

    if let Some(env) = env_with_exe {
        let exe = env.executable.as_ref().unwrap();

        // Cold resolve (first time)
        let (_, cold_time) = client.resolve(exe).expect("Failed to resolve (cold)");
        println!("Cold resolve time: {:?}", cold_time);

        // Warm resolve (cached)
        let (_, warm_time) = client.resolve(exe).expect("Failed to resolve (warm)");
        println!("Warm resolve time: {:?}", warm_time);

        // Warm should be faster than cold (if caching is working)
        if warm_time < cold_time {
            println!(
                "Cache speedup: {:.2}x",
                cold_time.as_micros() as f64 / warm_time.as_micros() as f64
            );
        }
    } else {
        println!("No environment with executable found");
    }
}

#[cfg_attr(feature = "ci-perf", test)]
#[allow(dead_code)]
fn test_concurrent_resolve_performance() {
    let mut client = PetClient::spawn().expect("Failed to spawn server");

    let cache_dir = get_test_cache_dir();
    let workspace_dir = get_workspace_dir();

    let config = json!({
        "workspaceDirectories": [workspace_dir],
        "cacheDirectory": cache_dir
    });

    client.configure(config).expect("Failed to configure");

    // First, discover environments
    client.refresh(None).expect("Failed to refresh");
    let environments = client.get_environments();

    // Get up to 5 environments with executables
    let exes: Vec<String> = environments
        .iter()
        .filter_map(|e| e.executable.clone())
        .take(5)
        .collect();

    if exes.is_empty() {
        println!("No environments with executables found");
        return;
    }

    println!("=== Sequential Resolve Performance ===");
    println!("Resolving {} executables sequentially", exes.len());

    let start = Instant::now();
    for exe in &exes {
        client.resolve(exe).expect("Failed to resolve");
    }
    let sequential_time = start.elapsed();
    println!("Sequential time: {:?}", sequential_time);
    println!(
        "Average per resolve: {:?}",
        sequential_time / exes.len() as u32
    );
}

#[cfg_attr(feature = "ci-perf", test)]
#[allow(dead_code)]
fn test_refresh_warm_vs_cold_cache() {
    // Clean cache directory
    let cache_dir = get_test_cache_dir();
    let _ = std::fs::remove_dir_all(&cache_dir);
    std::fs::create_dir_all(&cache_dir).expect("Failed to create cache dir");

    let workspace_dir = get_workspace_dir();

    println!("=== Cold vs Warm Cache Performance ===");

    // Cold cache test
    {
        let mut client = PetClient::spawn().expect("Failed to spawn server");
        let config = json!({
            "workspaceDirectories": [workspace_dir.clone()],
            "cacheDirectory": cache_dir.clone()
        });
        client.configure(config).expect("Failed to configure");

        let (result, elapsed) = client.refresh(None).expect("Failed to refresh");
        println!(
            "Cold cache: {}ms (server), {:?} (client)",
            result.duration, elapsed
        );
    }

    // Warm cache test (reuse same cache directory)
    {
        let mut client = PetClient::spawn().expect("Failed to spawn server");
        let config = json!({
            "workspaceDirectories": [workspace_dir],
            "cacheDirectory": cache_dir
        });
        client.configure(config).expect("Failed to configure");

        let (result, elapsed) = client.refresh(None).expect("Failed to refresh");
        println!(
            "Warm cache: {}ms (server), {:?} (client)",
            result.duration, elapsed
        );
    }
}

#[cfg_attr(feature = "ci-perf", test)]
#[allow(dead_code)]
fn test_performance_summary() {
    let mut metrics = PerformanceMetrics::default();

    let cache_dir = get_test_cache_dir();
    let _ = std::fs::remove_dir_all(&cache_dir);
    std::fs::create_dir_all(&cache_dir).expect("Failed to create cache dir");

    let workspace_dir = get_workspace_dir();

    // Measure server startup
    let spawn_start = Instant::now();
    let mut client = PetClient::spawn().expect("Failed to spawn server");

    let config = json!({
        "workspaceDirectories": [workspace_dir.clone()],
        "cacheDirectory": cache_dir
    });

    client.configure(config).expect("Failed to configure");
    metrics.server_startup_ms = spawn_start.elapsed().as_millis();

    // Measure full refresh
    let (result, _) = client.refresh(None).expect("Failed to refresh");
    metrics.full_refresh_ms = result.duration;
    metrics.environments_count = client.get_environments().len();
    metrics.managers_count = client.get_managers().len();

    if let Some(ttfe) = client.time_to_first_env() {
        metrics.time_to_first_env_ms = Some(ttfe.as_millis());
    }

    // Measure workspace refresh
    let (result, _) = client
        .refresh(Some(json!({ "searchPaths": [workspace_dir] })))
        .expect("Failed to refresh");
    metrics.workspace_refresh_ms = Some(result.duration);

    // Print summary
    println!("\n========================================");
    println!("       PERFORMANCE TEST SUMMARY         ");
    println!("========================================");
    println!("Server startup:        {}ms", metrics.server_startup_ms);
    println!("Full refresh:          {}ms", metrics.full_refresh_ms);
    if let Some(ws) = metrics.workspace_refresh_ms {
        println!("Workspace refresh:     {}ms", ws);
    }
    if let Some(ttfe) = metrics.time_to_first_env_ms {
        println!("Time to first env:     {}ms", ttfe);
    }
    println!("Environments found:    {}", metrics.environments_count);
    println!("Managers found:        {}", metrics.managers_count);
    println!("========================================\n");

    // Output as JSON for CI parsing
    let json_output = serde_json::to_string_pretty(&json!({
        "server_startup_ms": metrics.server_startup_ms,
        "full_refresh_ms": metrics.full_refresh_ms,
        "workspace_refresh_ms": metrics.workspace_refresh_ms,
        "time_to_first_env_ms": metrics.time_to_first_env_ms,
        "environments_count": metrics.environments_count,
        "managers_count": metrics.managers_count
    }))
    .unwrap();

    println!("JSON metrics:\n{}", json_output);
}
