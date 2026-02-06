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

/// Number of iterations for statistical tests
const STAT_ITERATIONS: usize = 10;

/// Statistical metrics with percentile calculations
#[derive(Debug, Clone, Default)]
pub struct StatisticalMetrics {
    samples: Vec<u128>,
}

impl StatisticalMetrics {
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }

    pub fn add(&mut self, value: u128) {
        self.samples.push(value);
    }

    pub fn count(&self) -> usize {
        self.samples.len()
    }

    pub fn min(&self) -> Option<u128> {
        self.samples.iter().copied().min()
    }

    pub fn max(&self) -> Option<u128> {
        self.samples.iter().copied().max()
    }

    pub fn mean(&self) -> Option<f64> {
        if self.samples.is_empty() {
            return None;
        }
        let sum: u128 = self.samples.iter().sum();
        Some(sum as f64 / self.samples.len() as f64)
    }

    pub fn std_dev(&self) -> Option<f64> {
        let mean = self.mean()?;
        if self.samples.len() < 2 {
            return None;
        }
        let variance: f64 = self
            .samples
            .iter()
            .map(|&x| {
                let diff = x as f64 - mean;
                diff * diff
            })
            .sum::<f64>()
            / (self.samples.len() - 1) as f64;
        Some(variance.sqrt())
    }

    fn sorted(&self) -> Vec<u128> {
        let mut sorted = self.samples.clone();
        sorted.sort();
        sorted
    }

    fn percentile(&self, p: f64) -> Option<u128> {
        if self.samples.is_empty() {
            return None;
        }
        let sorted = self.sorted();
        let n = sorted.len();
        if n == 1 {
            return Some(sorted[0]);
        }
        // Linear interpolation between closest ranks
        let rank = p / 100.0 * (n - 1) as f64;
        let lower = rank.floor() as usize;
        let upper = rank.ceil() as usize;
        let weight = rank - lower as f64;

        if upper >= n {
            return Some(sorted[n - 1]);
        }

        let result = sorted[lower] as f64 * (1.0 - weight) + sorted[upper] as f64 * weight;
        Some(result.round() as u128)
    }

    pub fn p50(&self) -> Option<u128> {
        self.percentile(50.0)
    }

    pub fn p95(&self) -> Option<u128> {
        self.percentile(95.0)
    }

    pub fn p99(&self) -> Option<u128> {
        self.percentile(99.0)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "count": self.count(),
            "min": self.min(),
            "max": self.max(),
            "mean": self.mean(),
            "std_dev": self.std_dev(),
            "p50": self.p50(),
            "p95": self.p95(),
            "p99": self.p99()
        })
    }

    pub fn print_summary(&self, label: &str) {
        println!(
            "{}: P50={}ms, P95={}ms, P99={}ms, mean={:.1}ms, std_dev={:.1}ms (n={})",
            label,
            self.p50().unwrap_or(0),
            self.p95().unwrap_or(0),
            self.p99().unwrap_or(0),
            self.mean().unwrap_or(0.0),
            self.std_dev().unwrap_or(0.0),
            self.count()
        );
    }
}

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

    let exe_name = if cfg!(windows) { "pet.exe" } else { "pet" };

    // When building with --target <triple>, cargo outputs to target/<triple>/release/
    // Check for target-specific builds first (used in CI)
    let target_triples = [
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-musl",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ];

    // Check target-specific release builds first
    for triple in target_triples {
        let target_release_exe = target_dir.join(triple).join("release").join(exe_name);
        if target_release_exe.exists() {
            return target_release_exe;
        }
    }

    // Fall back to standard release build (no --target flag)
    let release_exe = target_dir.join("release").join(exe_name);
    if release_exe.exists() {
        return release_exe;
    }

    // Check target-specific debug builds
    for triple in target_triples {
        let target_debug_exe = target_dir.join(triple).join("debug").join(exe_name);
        if target_debug_exe.exists() {
            return target_debug_exe;
        }
    }

    // Fall back to standard debug build
    target_dir.join("debug").join(exe_name)
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
    let mut spawn_stats = StatisticalMetrics::new();
    let mut configure_stats = StatisticalMetrics::new();
    let mut total_stats = StatisticalMetrics::new();

    let cache_dir = get_test_cache_dir();
    let workspace_dir = get_workspace_dir();

    println!(
        "=== Server Startup Performance ({} iterations) ===",
        STAT_ITERATIONS
    );

    for i in 0..STAT_ITERATIONS {
        let start = Instant::now();
        let mut client = PetClient::spawn().expect("Failed to spawn server");
        let spawn_time = start.elapsed();

        let config = json!({
            "workspaceDirectories": [workspace_dir.clone()],
            "cacheDirectory": cache_dir.clone()
        });

        let configure_time = client.configure(config).expect("Failed to configure");
        let total_time = spawn_time + configure_time;

        spawn_stats.add(spawn_time.as_millis());
        configure_stats.add(configure_time.as_millis());
        total_stats.add(total_time.as_millis());

        println!(
            "  Iteration {}: spawn={}ms, configure={}ms, total={}ms",
            i + 1,
            spawn_time.as_millis(),
            configure_time.as_millis(),
            total_time.as_millis()
        );
    }

    println!();
    spawn_stats.print_summary("Server spawn");
    configure_stats.print_summary("Configure");
    total_stats.print_summary("Total startup");

    // Output JSON for CI
    let json_output = serde_json::to_string_pretty(&json!({
        "spawn": spawn_stats.to_json(),
        "configure": configure_stats.to_json(),
        "total": total_stats.to_json()
    }))
    .unwrap();
    println!("\nJSON metrics:\n{}", json_output);

    // Assert reasonable startup time (P95 should be under 5 seconds)
    assert!(
        spawn_stats.p95().unwrap_or(0) < 5000,
        "Server spawn P95 took too long: {}ms",
        spawn_stats.p95().unwrap_or(0)
    );
    assert!(
        configure_stats.p95().unwrap_or(0) < 1000,
        "Configure P95 took too long: {}ms",
        configure_stats.p95().unwrap_or(0)
    );
}

#[cfg_attr(feature = "ci-perf", test)]
#[allow(dead_code)]
fn test_full_refresh_performance() {
    let mut server_duration_stats = StatisticalMetrics::new();
    let mut client_duration_stats = StatisticalMetrics::new();
    let mut time_to_first_env_stats = StatisticalMetrics::new();
    let mut env_count = 0usize;
    let mut manager_count = 0usize;
    let mut kind_counts: HashMap<String, usize> = HashMap::new();

    let cache_dir = get_test_cache_dir();
    let workspace_dir = get_workspace_dir();

    println!(
        "=== Full Refresh Performance ({} iterations) ===",
        STAT_ITERATIONS
    );

    for i in 0..STAT_ITERATIONS {
        // Fresh server each iteration for consistent cold-start measurement
        let mut client = PetClient::spawn().expect("Failed to spawn server");

        let config = json!({
            "workspaceDirectories": [workspace_dir.clone()],
            "cacheDirectory": cache_dir.clone()
        });

        client.configure(config).expect("Failed to configure");

        // Full machine refresh
        let (result, client_elapsed) = client.refresh(None).expect("Failed to refresh");
        let environments = client.get_environments();
        let managers = client.get_managers();

        server_duration_stats.add(result.duration);
        client_duration_stats.add(client_elapsed.as_millis());

        if let Some(time_to_first) = client.time_to_first_env() {
            time_to_first_env_stats.add(time_to_first.as_millis());
        }

        // Track counts from last iteration
        env_count = environments.len();
        manager_count = managers.len();

        // Aggregate kind counts
        if i == STAT_ITERATIONS - 1 {
            for env in &environments {
                if let Some(kind) = &env.kind {
                    *kind_counts.entry(kind.clone()).or_insert(0) += 1;
                }
            }
        }

        println!(
            "  Iteration {}: server={}ms, client={}ms, envs={}",
            i + 1,
            result.duration,
            client_elapsed.as_millis(),
            environments.len()
        );
    }

    println!();
    server_duration_stats.print_summary("Server duration");
    client_duration_stats.print_summary("Client duration");
    if time_to_first_env_stats.count() > 0 {
        time_to_first_env_stats.print_summary("Time to first env");
    }
    println!("Environments discovered: {}", env_count);
    println!("Managers discovered: {}", manager_count);
    println!("Environment kinds: {:?}", kind_counts);

    // Output JSON for CI
    let json_output = serde_json::to_string_pretty(&json!({
        "server_duration": server_duration_stats.to_json(),
        "client_duration": client_duration_stats.to_json(),
        "time_to_first_env": time_to_first_env_stats.to_json(),
        "environments_count": env_count,
        "managers_count": manager_count
    }))
    .unwrap();
    println!("\nJSON metrics:\n{}", json_output);

    // Assert we found at least some environments (CI should always have Python installed)
    assert!(
        env_count > 0,
        "No environments discovered - this is unexpected"
    );
}

#[cfg_attr(feature = "ci-perf", test)]
#[allow(dead_code)]
fn test_workspace_scoped_refresh_performance() {
    let mut server_duration_stats = StatisticalMetrics::new();
    let mut client_duration_stats = StatisticalMetrics::new();
    let mut env_count = 0usize;

    let cache_dir = get_test_cache_dir();
    let workspace_dir = get_workspace_dir();

    println!(
        "=== Workspace-Scoped Refresh Performance ({} iterations) ===",
        STAT_ITERATIONS
    );

    for i in 0..STAT_ITERATIONS {
        let mut client = PetClient::spawn().expect("Failed to spawn server");

        let config = json!({
            "workspaceDirectories": [workspace_dir.clone()],
            "cacheDirectory": cache_dir.clone()
        });

        client.configure(config).expect("Failed to configure");

        // Workspace-scoped refresh
        let (result, client_elapsed) = client
            .refresh(Some(json!({ "searchPaths": [workspace_dir.clone()] })))
            .expect("Failed to refresh");

        let environments = client.get_environments();

        server_duration_stats.add(result.duration);
        client_duration_stats.add(client_elapsed.as_millis());
        env_count = environments.len();

        println!(
            "  Iteration {}: server={}ms, client={}ms, envs={}",
            i + 1,
            result.duration,
            client_elapsed.as_millis(),
            environments.len()
        );
    }

    println!();
    server_duration_stats.print_summary("Server duration");
    client_duration_stats.print_summary("Client duration");
    println!("Environments discovered: {}", env_count);

    // Output JSON for CI
    let json_output = serde_json::to_string_pretty(&json!({
        "server_duration": server_duration_stats.to_json(),
        "client_duration": client_duration_stats.to_json(),
        "environments_count": env_count
    }))
    .unwrap();
    println!("\nJSON metrics:\n{}", json_output);
}

#[cfg_attr(feature = "ci-perf", test)]
#[allow(dead_code)]
fn test_kind_specific_refresh_performance() {
    let cache_dir = get_test_cache_dir();
    let workspace_dir = get_workspace_dir();

    // Test different environment kinds
    let kinds = ["Conda", "Venv", "VirtualEnv", "Pyenv"];

    println!(
        "=== Kind-Specific Refresh Performance ({} iterations per kind) ===",
        STAT_ITERATIONS
    );

    let mut all_kind_stats: HashMap<String, Value> = HashMap::new();

    for kind in kinds {
        let mut server_duration_stats = StatisticalMetrics::new();
        let mut env_count = 0usize;

        println!("\n  Testing kind: {}", kind);

        for i in 0..STAT_ITERATIONS {
            let mut client = PetClient::spawn().expect("Failed to spawn server");

            let config = json!({
                "workspaceDirectories": [workspace_dir.clone()],
                "cacheDirectory": cache_dir.clone()
            });

            client.configure(config).expect("Failed to configure");

            let (result, _) = client
                .refresh(Some(json!({ "searchKind": kind })))
                .expect(&format!("Failed to refresh for kind {}", kind));

            let environments = client.get_environments();
            server_duration_stats.add(result.duration);
            env_count = environments.len();

            println!(
                "    Iteration {}: {}ms, {} envs",
                i + 1,
                result.duration,
                environments.len()
            );
        }

        server_duration_stats.print_summary(&format!("  {}", kind));
        println!("  {} environments found: {}", kind, env_count);

        all_kind_stats.insert(
            kind.to_string(),
            json!({
                "duration": server_duration_stats.to_json(),
                "environments_count": env_count
            }),
        );
    }

    // Output JSON for CI
    let json_output = serde_json::to_string_pretty(&json!(all_kind_stats)).unwrap();
    println!("\nJSON metrics:\n{}", json_output);
}

#[cfg_attr(feature = "ci-perf", test)]
#[allow(dead_code)]
fn test_resolve_performance() {
    let mut cold_resolve_stats = StatisticalMetrics::new();
    let mut warm_resolve_stats = StatisticalMetrics::new();

    let cache_dir = get_test_cache_dir();
    let workspace_dir = get_workspace_dir();

    println!(
        "=== Resolve Performance ({} iterations) ===",
        STAT_ITERATIONS
    );

    // First, find an executable to test with (use a single server)
    let exe_to_test: String;
    {
        let mut client = PetClient::spawn().expect("Failed to spawn server");
        let config = json!({
            "workspaceDirectories": [workspace_dir.clone()],
            "cacheDirectory": cache_dir.clone()
        });
        client.configure(config).expect("Failed to configure");
        client.refresh(None).expect("Failed to refresh");
        let environments = client.get_environments();

        if environments.is_empty() {
            println!("No environments found to test resolve performance");
            return;
        }

        let env_with_exe = environments.iter().find(|e| e.executable.is_some());
        if let Some(env) = env_with_exe {
            exe_to_test = env.executable.as_ref().unwrap().clone();
        } else {
            println!("No environment with executable found");
            return;
        }
    }

    println!("Testing with executable: {}", exe_to_test);

    // Cold resolve tests (fresh server each time)
    println!("\n  Cold resolve iterations:");
    for i in 0..STAT_ITERATIONS {
        let mut client = PetClient::spawn().expect("Failed to spawn server");
        let config = json!({
            "workspaceDirectories": [workspace_dir.clone()],
            "cacheDirectory": cache_dir.clone()
        });
        client.configure(config).expect("Failed to configure");

        let (_, cold_time) = client
            .resolve(&exe_to_test)
            .expect("Failed to resolve (cold)");
        cold_resolve_stats.add(cold_time.as_millis());
        println!("    Iteration {}: {}ms", i + 1, cold_time.as_millis());
    }

    // Warm resolve tests (same server, multiple resolves)
    println!("\n  Warm resolve iterations:");
    {
        let mut client = PetClient::spawn().expect("Failed to spawn server");
        let config = json!({
            "workspaceDirectories": [workspace_dir.clone()],
            "cacheDirectory": cache_dir.clone()
        });
        client.configure(config).expect("Failed to configure");

        // Prime the cache with a first resolve
        client.resolve(&exe_to_test).expect("Failed to prime cache");

        for i in 0..STAT_ITERATIONS {
            let (_, warm_time) = client
                .resolve(&exe_to_test)
                .expect("Failed to resolve (warm)");
            warm_resolve_stats.add(warm_time.as_millis());
            println!("    Iteration {}: {}ms", i + 1, warm_time.as_millis());
        }
    }

    println!();
    cold_resolve_stats.print_summary("Cold resolve");
    warm_resolve_stats.print_summary("Warm resolve");

    // Calculate speedup
    if let (Some(cold_p50), Some(warm_p50)) = (cold_resolve_stats.p50(), warm_resolve_stats.p50()) {
        if warm_p50 > 0 {
            println!(
                "Cache speedup (P50): {:.2}x",
                cold_p50 as f64 / warm_p50 as f64
            );
        }
    }

    // Output JSON for CI
    let json_output = serde_json::to_string_pretty(&json!({
        "cold_resolve": cold_resolve_stats.to_json(),
        "warm_resolve": warm_resolve_stats.to_json()
    }))
    .unwrap();
    println!("\nJSON metrics:\n{}", json_output);
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
    let mut startup_stats = StatisticalMetrics::new();
    let mut refresh_stats = StatisticalMetrics::new();
    let mut time_to_first_env_stats = StatisticalMetrics::new();
    let mut env_count = 0usize;
    let mut manager_count = 0usize;

    let cache_dir = get_test_cache_dir();
    let _ = std::fs::remove_dir_all(&cache_dir);
    std::fs::create_dir_all(&cache_dir).expect("Failed to create cache dir");

    let workspace_dir = get_workspace_dir();

    println!("\n========================================");
    println!("  PERFORMANCE SUMMARY ({} iterations)", STAT_ITERATIONS);
    println!("========================================\n");

    for i in 0..STAT_ITERATIONS {
        // Measure server startup (fresh server each iteration)
        let spawn_start = Instant::now();
        let mut client = PetClient::spawn().expect("Failed to spawn server");

        let config = json!({
            "workspaceDirectories": [workspace_dir.clone()],
            "cacheDirectory": cache_dir.clone()
        });

        client.configure(config).expect("Failed to configure");
        let startup_time = spawn_start.elapsed().as_millis();
        startup_stats.add(startup_time);

        // Measure full refresh
        let (result, _) = client.refresh(None).expect("Failed to refresh");
        refresh_stats.add(result.duration);

        env_count = client.get_environments().len();
        manager_count = client.get_managers().len();

        if let Some(ttfe) = client.time_to_first_env() {
            time_to_first_env_stats.add(ttfe.as_millis());
        }

        println!(
            "  Iteration {}: startup={}ms, refresh={}ms, envs={}",
            i + 1,
            startup_time,
            result.duration,
            env_count
        );
    }

    // Print statistical summary
    println!("\n----------------------------------------");
    println!("             STATISTICS                 ");
    println!("----------------------------------------");
    startup_stats.print_summary("Server startup");
    refresh_stats.print_summary("Full refresh");
    if time_to_first_env_stats.count() > 0 {
        time_to_first_env_stats.print_summary("Time to first env");
    }
    println!("Environments found:    {}", env_count);
    println!("Managers found:        {}", manager_count);
    println!("========================================\n");

    // Output as JSON for CI parsing
    // Includes both P50 values at top level (for backwards compatibility) and full stats
    let json_output = serde_json::to_string_pretty(&json!({
        "server_startup_ms": startup_stats.p50().unwrap_or(0),
        "full_refresh_ms": refresh_stats.p50().unwrap_or(0),
        "time_to_first_env_ms": time_to_first_env_stats.p50(),
        "environments_count": env_count,
        "managers_count": manager_count,
        "stats": {
            "server_startup": startup_stats.to_json(),
            "full_refresh": refresh_stats.to_json(),
            "time_to_first_env": time_to_first_env_stats.to_json()
        }
    }))
    .unwrap();

    println!("JSON metrics:\n{}", json_output);
}
