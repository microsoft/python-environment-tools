// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_fs::path::norm_case;
use serde_json::json;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[allow(dead_code)]
mod jsonrpc_client;
use jsonrpc_client::{
    EnvironmentNotification, JsonRpcNotification, PendingRequest, PetJsonRpcClient,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const FAST_SIZES: &[usize] = &[1, 10, 100];
const STRESS_SIZES: &[usize] = &[1, 10, 100, 1000];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EnvironmentIdentity {
    executable: PathBuf,
    prefix: PathBuf,
    kind: String,
    name: Option<String>,
    version: Option<String>,
}

#[derive(Debug, Clone)]
struct ResourceSample {
    resident_bytes: u64,
    threads: Option<usize>,
    descriptors: Option<usize>,
}

struct RefreshMeasurement {
    inventory: Vec<EnvironmentIdentity>,
    round_trip_us: u128,
    ttfe_us: u128,
    ambient_environment_count: usize,
    ambient_manager_count: usize,
}

struct Fixture {
    _root: TempDir,
    workspace: PathBuf,
    cache: PathBuf,
    barrier: PathBuf,
    python_path: PathBuf,
    resolve_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolveFixtureIdentity {
    executable: PathBuf,
    prefix: PathBuf,
    version: String,
}

struct PendingFixtureResolve {
    request: PendingRequest,
    expected: ResolveFixtureIdentity,
}

impl Fixture {
    fn new() -> Self {
        let resolve_python = session_python();
        let resolve_version = runtime_version(&resolve_python);
        let root = tempfile::tempdir().expect("failed to create session fixture root");
        let workspace = root.path().join("workspace");
        let cache = root.path().join("cache");
        let barrier = root.path().join("resolve-barrier");
        let python_path = root.path().join("python-path");
        fs::create_dir_all(&workspace).expect("failed to create fixture workspace");
        fs::create_dir_all(&cache).expect("failed to create fixture cache");
        fs::create_dir_all(&barrier).expect("failed to create resolve barrier");
        fs::create_dir_all(&python_path).expect("failed to create fixture PYTHONPATH");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("session_sitecustomize.py"),
            python_path.join("sitecustomize.py"),
        )
        .expect("failed to install resolve barrier module");
        Self {
            _root: root,
            workspace,
            cache,
            barrier,
            python_path,
            resolve_version,
        }
    }

    fn reset_inventory(&self, size: usize) -> Vec<EnvironmentIdentity> {
        self.reset_inventory_at(&self.workspace, size)
    }

    fn reset_inventory_at(&self, workspace: &Path, size: usize) -> Vec<EnvironmentIdentity> {
        if workspace.exists() {
            fs::remove_dir_all(workspace).expect("failed to clear fixture workspace");
        }
        fs::create_dir_all(workspace).expect("failed to recreate fixture workspace");
        (0..size)
            .map(|index| {
                self.create_fake_environment_at(workspace, &format!("env-{index:04}"), "3.11.0")
            })
            .collect()
    }

    fn create_fake_environment(&self, name: &str, version: &str) -> EnvironmentIdentity {
        self.create_fake_environment_at(&self.workspace, name, version)
    }

    fn create_fake_environment_at(
        &self,
        workspace: &Path,
        name: &str,
        version: &str,
    ) -> EnvironmentIdentity {
        let prefix = workspace.join(name);
        let bin = bin_directory(&prefix);
        fs::create_dir_all(&bin).expect("failed to create fake environment");
        fs::write(
            prefix.join("pyvenv.cfg"),
            format!("version = {version}\nprompt = {name}\n"),
        )
        .expect("failed to write pyvenv.cfg");
        write_version_header(&prefix, version);
        let executable = python_executable(&bin, false);
        fs::write(&executable, b"fixture").expect("failed to create fake Python");
        EnvironmentIdentity {
            executable: norm_case(executable),
            prefix: norm_case(prefix),
            kind: "Venv".to_string(),
            name: Some(name.to_string()),
            version: Some(version.to_string()),
        }
    }

    fn create_resolve_environments(&self, count: usize) -> Vec<ResolveFixtureIdentity> {
        (0..count)
            .map(|index| {
                self.create_resolve_environment(
                    &self.resolve_root().join(format!("resolve-{index}")),
                )
            })
            .collect()
    }

    fn create_resolve_environment(&self, prefix: &Path) -> ResolveFixtureIdentity {
        let python = session_python();
        let output = Command::new(&python)
            .args(["-m", "venv", "--without-pip", "--copies"])
            .arg(prefix)
            .output()
            .expect("failed to start Python venv fixture setup");
        assert!(
            output.status.success(),
            "failed to create resolve environment at {}: {}",
            prefix.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        ResolveFixtureIdentity {
            executable: resolve_fixture_path(&python_executable(&bin_directory(prefix), false)),
            prefix: resolve_fixture_path(prefix),
            version: self.resolve_version.clone(),
        }
    }

    fn resolve_root(&self) -> PathBuf {
        self._root.path().join("resolve-environments")
    }

    fn clear_barrier(&self) {
        for entry in fs::read_dir(&self.barrier).expect("failed to read resolve barrier") {
            let path = entry.expect("failed to read barrier entry").path();
            fs::remove_file(path).expect("failed to clear resolve barrier entry");
        }
    }

    fn reset_cache(&self) {
        if self.cache.exists() {
            fs::remove_dir_all(&self.cache).expect("failed to clear fixture cache");
        }
        fs::create_dir_all(&self.cache).expect("failed to recreate fixture cache");
    }

    fn entered_count(&self) -> usize {
        fs::read_dir(&self.barrier)
            .expect("failed to read resolve barrier")
            .map(|entry| entry.expect("failed to read resolve barrier entry"))
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("entered-"))
            .count()
    }

    fn released_count(&self) -> usize {
        fs::read_dir(&self.barrier)
            .expect("failed to read resolve barrier")
            .map(|entry| entry.expect("failed to read resolve barrier entry"))
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("released-"))
            .count()
    }

    fn failed_count(&self) -> usize {
        fs::read_dir(&self.barrier)
            .expect("failed to read resolve barrier")
            .map(|entry| entry.expect("failed to read resolve barrier entry"))
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("failed-"))
            .count()
    }

    fn wait_for_entered(&self, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if self.entered_count() == expected {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "only {} of {expected} resolve subprocesses entered the barrier",
            self.entered_count()
        );
    }
}

fn session_python() -> OsString {
    std::env::var_os("PET_SESSION_PYTHON").unwrap_or_else(|| {
        if cfg!(windows) {
            "python".into()
        } else {
            "python3".into()
        }
    })
}

fn runtime_version(python: &OsString) -> String {
    let output = Command::new(python)
        .args([
            "-I",
            "-c",
            "import sys; print('.'.join(str(part) for part in sys.version_info))",
        ])
        .output()
        .expect("failed to query Python fixture runtime version");
    assert!(
        output.status.success(),
        "failed to query Python fixture runtime version: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("Python fixture runtime version was not UTF-8")
        .trim()
        .to_string()
}

fn resolve_fixture_path(path: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    let path = fs::canonicalize(path).expect("failed to resolve macOS fixture path");

    norm_case(path)
}

struct BarrierReleaseGuard {
    release: PathBuf,
    released: bool,
}

impl BarrierReleaseGuard {
    fn new(barrier: &Path) -> Self {
        Self {
            release: barrier.join("release"),
            released: false,
        }
    }

    fn release(&mut self) {
        fs::write(&self.release, b"release").expect("failed to release resolve fixture");
        self.released = true;
    }
}

impl Drop for BarrierReleaseGuard {
    fn drop(&mut self) {
        if !self.released {
            let _ = fs::write(&self.release, b"release");
        }
    }
}

fn bin_directory(prefix: &Path) -> PathBuf {
    if cfg!(windows) {
        prefix.join("Scripts")
    } else {
        prefix.join("bin")
    }
}

fn write_version_header(prefix: &Path, version: &str) {
    let include = prefix.join("include");
    fs::create_dir_all(&include).expect("failed to create fixture include directory");
    fs::write(
        include.join("patchlevel.h"),
        format!("#define PY_VERSION \"{version}\"\n"),
    )
    .expect("failed to write fixture Python version header");
}

fn python_executable(bin: &Path, alias: bool) -> PathBuf {
    match (cfg!(windows), alias) {
        (true, false) => bin.join("python.exe"),
        (true, true) => bin.join("python3.exe"),
        (false, false) => bin.join("python"),
        (false, true) => bin.join("python3"),
    }
}

fn fixture_inventory(
    notifications: Vec<EnvironmentNotification>,
    workspace: &Path,
) -> (Vec<EnvironmentIdentity>, usize) {
    let workspace = norm_case(workspace);
    let (fixture, ambient): (Vec<_>, Vec<_>) = notifications.into_iter().partition(|environment| {
        environment
            .prefix
            .as_ref()
            .is_some_and(|prefix| norm_case(prefix).starts_with(&workspace))
    });
    let mut identities = fixture
        .into_iter()
        .map(|environment| {
            assert_eq!(
                environment.error, None,
                "fixture environment reported an error"
            );
            EnvironmentIdentity {
                executable: norm_case(
                    environment
                        .executable
                        .expect("fixture environment had no executable"),
                ),
                prefix: norm_case(
                    environment
                        .prefix
                        .expect("fixture environment had no prefix"),
                ),
                kind: environment
                    .kind
                    .expect("fixture environment had no classification"),
                name: environment.name,
                version: environment.version,
            }
        })
        .collect::<Vec<_>>();
    identities.sort_unstable();
    (identities, ambient.len())
}

fn manager_counts(client: &PetJsonRpcClient, workspace: &Path) -> (usize, usize) {
    let workspace = norm_case(workspace);
    let (fixture, ambient): (Vec<_>, Vec<_>) = client
        .manager_notifications()
        .into_iter()
        .partition(|manager| {
            manager
                .executable
                .as_ref()
                .is_some_and(|executable| norm_case(executable).starts_with(&workspace))
        });
    (fixture.len(), ambient.len())
}

fn first_fixture_environment(
    notifications: &[JsonRpcNotification],
    workspace: &Path,
) -> Option<Instant> {
    let workspace = norm_case(workspace);
    notifications
        .iter()
        .filter(|notification| {
            notification.method == "environment"
                && notification.params["prefix"]
                    .as_str()
                    .is_some_and(|prefix| norm_case(prefix).starts_with(&workspace))
        })
        .map(|notification| notification.received_at)
        .min()
}

#[test]
fn first_result_timing_ignores_ambient_environments_and_managers() {
    let workspace = Path::new("workspace");
    let start = Instant::now();
    let notification = |method: &str, prefix: Option<PathBuf>, millis| JsonRpcNotification {
        method: method.to_string(),
        params: json!({ "prefix": prefix }),
        received_at: start + Duration::from_millis(millis),
    };
    let mut notifications = vec![
        notification("environment", None, 1),
        notification("environment", Some(PathBuf::from("workspace-other")), 2),
        notification("manager", Some(workspace.join("manager")), 3),
    ];
    assert_eq!(first_fixture_environment(&notifications, workspace), None);
    notifications.push(notification(
        "environment",
        Some(workspace.join("second")),
        20,
    ));
    notifications.push(notification(
        "environment",
        Some(workspace.join("first")),
        10,
    ));
    assert_eq!(
        first_fixture_environment(&notifications, workspace),
        Some(start + Duration::from_millis(10))
    );
}

fn refresh_and_measure(client: &PetJsonRpcClient, workspace: &Path) -> RefreshMeasurement {
    client.clear_notifications();
    let timing = client
        .refresh_with_timing(Some(json!({ "searchPaths": [workspace] })))
        .expect("session refresh failed");
    let first_environment = first_fixture_environment(&client.notifications(), workspace)
        .expect("fixture refresh produced no environment");
    let ttfe = first_environment
        .checked_duration_since(timing.submitted_at)
        .expect("environment notification preceded refresh submission");
    assert!(
        ttfe <= timing.round_trip,
        "time-to-first environment exceeded refresh round trip"
    );
    let _server_duration = timing.result.duration;
    let (inventory, ambient_environment_count) =
        fixture_inventory(client.environment_notifications(), workspace);
    let (fixture_manager_count, ambient_manager_count) = manager_counts(client, workspace);
    assert_eq!(
        fixture_manager_count, 0,
        "fixture workspace unexpectedly reported an environment manager"
    );
    RefreshMeasurement {
        inventory,
        round_trip_us: timing.round_trip.as_micros(),
        ttfe_us: ttfe.as_micros(),
        ambient_environment_count,
        ambient_manager_count,
    }
}

fn directory_usage(root: &Path) -> (usize, u64) {
    let mut pending = vec![root.to_path_buf()];
    let mut files = 0;
    let mut bytes = 0;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).expect("failed to read cache directory") {
            let entry = entry.expect("failed to read cache entry");
            let metadata = entry.metadata().expect("failed to read cache metadata");
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                files += 1;
                bytes += metadata.len();
            }
        }
    }
    (files, bytes)
}

fn cache_contents(root: &Path) -> io::Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut pending = vec![(PathBuf::new(), root.to_path_buf())];
    let mut contents = BTreeMap::new();
    while let Some((relative_directory, directory)) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let relative_path = relative_directory.join(entry.file_name());
            if file_type.is_dir() {
                pending.push((relative_path, entry.path()));
            } else if file_type.is_file() {
                contents.insert(relative_path, fs::read(entry.path())?);
            }
        }
    }
    Ok(contents)
}

fn original_cache_entries_unchanged(
    original: &BTreeMap<PathBuf, Vec<u8>>,
    current: &BTreeMap<PathBuf, Vec<u8>>,
) -> bool {
    original
        .iter()
        .all(|(path, bytes)| current.get(path) == Some(bytes))
}

#[test]
fn cache_preservation_allows_additions_but_rejects_original_entry_changes() {
    let original = BTreeMap::from([
        (PathBuf::from("flat.json"), b"original".to_vec()),
        (
            PathBuf::from("nested").join("entry.json"),
            b"nested".to_vec(),
        ),
    ]);
    let mut with_addition = original.clone();
    with_addition.insert(PathBuf::from("ambient.json"), b"ambient".to_vec());
    assert!(original_cache_entries_unchanged(&original, &with_addition));

    let mut changed = with_addition.clone();
    changed.insert(PathBuf::from("flat.json"), b"modified".to_vec());
    assert!(!original_cache_entries_unchanged(&original, &changed));

    let mut deleted = with_addition;
    deleted.remove(&PathBuf::from("nested").join("entry.json"));
    assert!(!original_cache_entries_unchanged(&original, &deleted));
}

fn observe_resources(pid: u32) -> ResourceSample {
    let samples = (0..3)
        .map(|_| {
            let sample = sample_process(pid);
            thread::sleep(Duration::from_millis(20));
            sample
        })
        .collect::<Vec<_>>();
    ResourceSample {
        resident_bytes: samples
            .iter()
            .map(|sample| sample.resident_bytes)
            .max()
            .unwrap(),
        threads: samples.iter().filter_map(|sample| sample.threads).max(),
        descriptors: samples.iter().filter_map(|sample| sample.descriptors).max(),
    }
}

fn observed_resource_peak(samples: &[ResourceSample]) -> ResourceSample {
    ResourceSample {
        resident_bytes: samples
            .iter()
            .map(|sample| sample.resident_bytes)
            .max()
            .expect("at least one resource sample is required"),
        threads: samples.iter().filter_map(|sample| sample.threads).max(),
        descriptors: samples.iter().filter_map(|sample| sample.descriptors).max(),
    }
}

fn resource_json(sample: &ResourceSample) -> serde_json::Value {
    json!({
        "residentBytes": sample.resident_bytes,
        "threads": sample.threads,
        "handlesOrDescriptors": sample.descriptors,
    })
}

fn cache_json((files, bytes): (usize, u64)) -> serde_json::Value {
    json!({
        "files": files,
        "bytes": bytes,
    })
}

fn extract_resolved_fixture(
    result: serde_json::Value,
    expected: &ResolveFixtureIdentity,
) -> Result<EnvironmentIdentity, String> {
    let environment: EnvironmentNotification = serde_json::from_value(result)
        .map_err(|error| format!("resolve returned an invalid environment: {error}"))?;
    if let Some(error) = environment.error {
        return Err(format!("resolve reported an error: {error}"));
    }
    let executable = environment
        .executable
        .ok_or_else(|| "resolved environment had no executable".to_string())?;
    let prefix = environment
        .prefix
        .ok_or_else(|| "resolved environment had no prefix".to_string())?;
    let kind = environment
        .kind
        .ok_or_else(|| "resolved environment had no classification".to_string())?;
    let version = environment
        .version
        .ok_or_else(|| "resolved environment had no version".to_string())?;
    let identity = EnvironmentIdentity {
        executable: resolve_fixture_path(Path::new(&executable)),
        prefix: resolve_fixture_path(Path::new(&prefix)),
        kind,
        name: environment.name,
        version: Some(version),
    };
    if identity.executable != expected.executable {
        return Err("resolve returned a different fixture executable".to_string());
    }
    if identity.prefix != expected.prefix {
        return Err("resolve returned a different fixture prefix".to_string());
    }
    if identity.kind != "Venv" {
        return Err(format!(
            "resolve returned kind {}, expected Venv",
            identity.kind
        ));
    }
    if identity.version.as_deref() != Some(expected.version.as_str()) {
        return Err("resolve returned an unexpected runtime version".to_string());
    }
    Ok(identity)
}

fn submit_fixture_resolve(
    client: &PetJsonRpcClient,
    expected: &ResolveFixtureIdentity,
) -> PendingFixtureResolve {
    let executable = expected
        .executable
        .to_str()
        .expect("resolve fixture path was not UTF-8");
    PendingFixtureResolve {
        request: client
            .submit_resolve(executable)
            .expect("failed to submit fixture resolve"),
        expected: expected.clone(),
    }
}

impl PendingFixtureResolve {
    fn submitted_at(&self) -> Instant {
        self.request.submitted_at()
    }

    fn wait(self, context: &str) -> (EnvironmentIdentity, Duration) {
        let (result, latency) = self
            .request
            .wait(REQUEST_TIMEOUT)
            .unwrap_or_else(|error| panic!("{context} failed: {error}"));
        let identity = extract_resolved_fixture(result, &self.expected)
            .unwrap_or_else(|error| panic!("{context} returned the wrong environment: {error}"));
        (identity, latency)
    }
}

fn resolve_and_measure(
    client: &PetJsonRpcClient,
    expected: &ResolveFixtureIdentity,
) -> (EnvironmentIdentity, u128) {
    let (identity, latency) =
        submit_fixture_resolve(client, expected).wait("cache-control resolve");
    (identity, latency.as_micros())
}

#[test]
fn resolved_fixture_validation_enforces_request_identity_and_response_shape() {
    let root = tempfile::tempdir().expect("failed to create resolve validation fixture");
    let create_expected = |name: &str, version: &str| {
        let prefix = root.path().join(name);
        let executable = python_executable(&bin_directory(&prefix), false);
        fs::create_dir_all(executable.parent().unwrap())
            .expect("failed to create resolve validation environment");
        fs::write(&executable, b"fixture").expect("failed to create resolve validation executable");
        ResolveFixtureIdentity {
            executable: resolve_fixture_path(&executable),
            prefix: resolve_fixture_path(&prefix),
            version: version.to_string(),
        }
    };
    let first = create_expected("first", "3.12.10.final.0");
    let second = create_expected("second", "3.13.2.final.0");
    let response = |expected: &ResolveFixtureIdentity| {
        json!({
            "executable": expected.executable,
            "prefix": expected.prefix,
            "kind": "Venv",
            "name": "fixture",
            "version": expected.version,
            "error": null,
        })
    };

    let valid = extract_resolved_fixture(response(&first), &first)
        .expect("valid resolve response was rejected");
    assert_eq!(valid.executable, first.executable);
    assert_eq!(valid.prefix, first.prefix);
    assert_eq!(valid.kind, "Venv");
    assert_eq!(valid.version.as_deref(), Some(first.version.as_str()));

    assert!(
        extract_resolved_fixture(response(&second), &first).is_err(),
        "a response assigned to the wrong requested fixture was accepted"
    );
    assert!(
        extract_resolved_fixture(json!(["not", "an", "environment"]), &first).is_err(),
        "malformed resolve JSON was accepted"
    );
    assert!(
        extract_resolved_fixture(json!({ "error": null }), &first).is_err(),
        "a resolve response with missing identity fields was accepted"
    );

    let mut errored = response(&first);
    errored["error"] = json!("probe failed");
    assert!(
        extract_resolved_fixture(errored, &first).is_err(),
        "a resolve response containing an error was accepted"
    );

    let mut wrong_kind = response(&first);
    wrong_kind["kind"] = json!("VirtualEnv");
    assert!(
        extract_resolved_fixture(wrong_kind, &first).is_err(),
        "a resolve response with the wrong kind was accepted"
    );

    let mut wrong_version = response(&first);
    wrong_version["version"] = json!("3.12.10");
    assert!(
        extract_resolved_fixture(wrong_version, &first).is_err(),
        "a resolve response with the wrong runtime version was accepted"
    );
}

fn shutdown_client(client: &PetJsonRpcClient, context: &str) {
    let status = client
        .shutdown(Duration::from_secs(10))
        .unwrap_or_else(|error| panic!("{context} did not shut down gracefully: {error}"));
    assert!(
        status.success(),
        "{context} exited unsuccessfully with {status}"
    );
}

#[cfg(target_os = "linux")]
fn sample_process(pid: u32) -> ResourceSample {
    let status = fs::read_to_string(format!("/proc/{pid}/status"))
        .expect("failed to read process-specific Linux status");
    let value = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_else(|| panic!("Linux process status omitted {name}"))
    };
    ResourceSample {
        resident_bytes: value("VmRSS:") * 1024,
        threads: Some(value("Threads:") as usize),
        descriptors: Some(
            fs::read_dir(format!("/proc/{pid}/fd"))
                .expect("failed to read process-specific descriptor directory")
                .try_fold(0, |count, entry| entry.map(|_| count + 1))
                .expect("failed to read process descriptor entry"),
        ),
    }
}

#[cfg(windows)]
fn sample_process(pid: u32) -> ResourceSample {
    let script = format!(
        "$p=Get-Process -Id {pid} -ErrorAction Stop; \
         Write-Output \"$($p.WorkingSet64),$($p.Threads.Count),$($p.HandleCount)\""
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .expect("failed to sample PET process");
    assert!(
        output.status.success(),
        "process-specific Windows sampling failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let fields = String::from_utf8(output.stdout)
        .expect("Windows resource sample was not UTF-8")
        .trim()
        .split(',')
        .map(|field| {
            field
                .parse::<u64>()
                .expect("invalid Windows resource field")
        })
        .collect::<Vec<_>>();
    assert_eq!(fields.len(), 3);
    ResourceSample {
        resident_bytes: fields[0],
        threads: Some(fields[1] as usize),
        descriptors: Some(fields[2] as usize),
    }
}

#[cfg(target_os = "macos")]
fn sample_process(pid: u32) -> ResourceSample {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .expect("failed to sample PET process");
    assert!(
        output.status.success(),
        "process-specific macOS sampling failed"
    );
    let fields = String::from_utf8(output.stdout)
        .expect("macOS resource sample was not UTF-8")
        .split_whitespace()
        .map(|field| field.parse::<u64>().expect("invalid macOS resource field"))
        .collect::<Vec<_>>();
    assert_eq!(fields.len(), 1);
    ResourceSample {
        resident_bytes: fields[0] * 1024,
        threads: None,
        descriptors: None,
    }
}

#[cfg_attr(feature = "ci-perf", test)]
#[allow(dead_code)]
fn long_lived_session_benchmark() {
    let stress = std::env::var("PET_SESSION_STRESS").is_ok_and(|value| value == "1");
    let sizes = if stress { STRESS_SIZES } else { FAST_SIZES };
    let samples_per_size = if stress { 10 } else { 3 };
    let resolve_concurrency = if stress { 10 } else { 4 };
    let cold_resolve_batches = if stress { 5 } else { 2 };
    let fixture = Fixture::new();
    let mut refresh_scenario_samples = Vec::new();
    let mut cache_usage = Vec::new();
    let mut inventory_resource_samples: Vec<(usize, ResourceSample)> = Vec::new();
    let mut max_ambient_environment_count = 0;
    let mut max_ambient_manager_count = 0;

    let barrier = fixture.barrier.as_os_str();
    let python_path = fixture.python_path.as_os_str();
    let resolve_root = fixture.resolve_root();
    let resolve_environment =
        fixture.create_resolve_environment(&resolve_root.join("cache-control"));
    let resolve_cache = fixture._root.path().join("resolve-cache");
    fs::create_dir_all(&resolve_cache).expect("failed to create resolve cache");
    let resolve_environment_variables = [
        ("PET_SESSION_RESOLVE_BARRIER", barrier),
        ("PET_SESSION_RESOLVE_ROOT", resolve_root.as_os_str()),
        ("PYTHONPATH", python_path),
    ];

    fixture.clear_barrier();
    let mut cold_release = BarrierReleaseGuard::new(&fixture.barrier);
    cold_release.release();
    let cold_process = PetJsonRpcClient::spawn_with_environment(&resolve_environment_variables)
        .expect("failed to spawn cold cache-control server");
    cold_process
        .configure(json!({
            "workspaceDirectories": [&fixture.workspace],
            "cacheDirectory": &resolve_cache,
        }))
        .expect("failed to configure cold cache-control server");
    let (cold_identity, cold_latency_us) = resolve_and_measure(&cold_process, &resolve_environment);
    assert_eq!(
        fixture.entered_count(),
        1,
        "cold cache-control resolve must probe the interpreter once"
    );
    assert_eq!(fixture.released_count(), 1);
    assert_eq!(fixture.failed_count(), 0);
    shutdown_client(&cold_process, "cold cache-control server");
    let cache_after_cold_resolve = directory_usage(&resolve_cache);
    assert!(
        cache_after_cold_resolve.0 > 0 && cache_after_cold_resolve.1 > 0,
        "cold real resolve did not produce a persistent cache entry"
    );

    fixture.clear_barrier();
    let mut warm_release = BarrierReleaseGuard::new(&fixture.barrier);
    warm_release.release();
    let warm_process = PetJsonRpcClient::spawn_with_environment(&resolve_environment_variables)
        .expect("failed to spawn disk-warm cache-control server");
    warm_process
        .configure(json!({
            "workspaceDirectories": [&fixture.workspace],
            "cacheDirectory": &resolve_cache,
        }))
        .expect("failed to configure disk-warm cache-control server");
    let (disk_warm_identity, disk_warm_latency_us) =
        resolve_and_measure(&warm_process, &resolve_environment);
    assert_eq!(
        fixture.entered_count(),
        0,
        "disk-warm resolve spawned an interpreter instead of using the persistent cache"
    );
    let (same_process_identity, same_process_warm_latency_us) =
        resolve_and_measure(&warm_process, &resolve_environment);
    assert_eq!(
        fixture.entered_count(),
        0,
        "same-process warm resolve unexpectedly spawned an interpreter"
    );
    assert_eq!(cold_identity, disk_warm_identity);
    assert_eq!(cold_identity, same_process_identity);
    shutdown_client(&warm_process, "disk-warm cache-control server");
    let persistent_cache_resolve_samples = vec![
        json!({
            "scenario": "cold",
            "sampleCount": 1,
            "latencyUs": [cold_latency_us],
            "interpreterProcessesStarted": 1,
        }),
        json!({
            "scenario": "diskWarm",
            "sampleCount": 1,
            "latencyUs": [disk_warm_latency_us],
            "interpreterProcessesStarted": 0,
        }),
        json!({
            "scenario": "sameProcessWarm",
            "sampleCount": 1,
            "latencyUs": [same_process_warm_latency_us],
            "interpreterProcessesStarted": 0,
        }),
    ];

    for &size in sizes {
        let mut expected = fixture.reset_inventory(size);
        expected.sort_unstable();

        fixture.reset_cache();
        let before_first_process = directory_usage(&fixture.cache);
        assert_eq!(
            before_first_process,
            (0, 0),
            "first-process scenario must start with an empty disk cache"
        );
        let first_process =
            PetJsonRpcClient::spawn().expect("failed to spawn first-process scenario server");
        first_process
            .configure(json!({
                "workspaceDirectories": [&fixture.workspace],
                "cacheDirectory": &fixture.cache,
            }))
            .expect("failed to configure first-process scenario server");
        let first = refresh_and_measure(&first_process, &fixture.workspace);
        assert_eq!(
            first.inventory, expected,
            "first-process refresh changed fixture identities at size {size}"
        );
        max_ambient_environment_count =
            max_ambient_environment_count.max(first.ambient_environment_count);
        max_ambient_manager_count = max_ambient_manager_count.max(first.ambient_manager_count);
        refresh_scenario_samples.push(json!({
            "scenario": "firstProcessEmptyDiskCache",
            "inventorySize": size,
            "sampleCount": 1,
            "roundTripUs": [first.round_trip_us],
            "ttfeUs": [first.ttfe_us],
        }));
        shutdown_client(&first_process, "first-process scenario server");

        let after_first_refresh = directory_usage(&fixture.cache);
        let new_process =
            PetJsonRpcClient::spawn().expect("failed to spawn reused-cache scenario server");
        new_process
            .configure(json!({
                "workspaceDirectories": [&fixture.workspace],
                "cacheDirectory": &fixture.cache,
            }))
            .expect("failed to configure reused-cache scenario server");
        let reused = refresh_and_measure(&new_process, &fixture.workspace);
        assert_eq!(
            reused.inventory, expected,
            "new-process refresh changed fixture identities at size {size}"
        );
        max_ambient_environment_count =
            max_ambient_environment_count.max(reused.ambient_environment_count);
        max_ambient_manager_count = max_ambient_manager_count.max(reused.ambient_manager_count);
        refresh_scenario_samples.push(json!({
            "scenario": "newProcessAfterFirstRefresh",
            "inventorySize": size,
            "sampleCount": 1,
            "roundTripUs": [reused.round_trip_us],
            "ttfeUs": [reused.ttfe_us],
        }));

        let mut warm_round_trip_us = Vec::with_capacity(samples_per_size);
        let mut warm_ttfe_us = Vec::with_capacity(samples_per_size);
        for _ in 0..samples_per_size {
            let warm = refresh_and_measure(&new_process, &fixture.workspace);
            assert_eq!(
                warm.inventory, expected,
                "same-process warm refresh changed fixture identities at size {size}"
            );
            max_ambient_environment_count =
                max_ambient_environment_count.max(warm.ambient_environment_count);
            max_ambient_manager_count = max_ambient_manager_count.max(warm.ambient_manager_count);
            warm_round_trip_us.push(warm.round_trip_us);
            warm_ttfe_us.push(warm.ttfe_us);
        }
        refresh_scenario_samples.push(json!({
            "scenario": "sameProcessWarm",
            "inventorySize": size,
            "sampleCount": samples_per_size,
            "roundTripUs": warm_round_trip_us,
            "ttfeUs": warm_ttfe_us,
        }));
        inventory_resource_samples.push((size, observe_resources(new_process.process_id())));
        shutdown_client(&new_process, "reused-cache scenario server");

        let after_warm_refresh = directory_usage(&fixture.cache);
        cache_usage.push(json!({
            "inventorySize": size,
            "beforeFirstProcess": cache_json(before_first_process),
            "afterFirstRefresh": cache_json(after_first_refresh),
            "afterWarmRefresh": cache_json(after_warm_refresh),
            "diskCacheAvailableForNewProcess": after_first_refresh.1 > 0,
        }));
    }

    let client = PetJsonRpcClient::spawn_with_environment(&[
        ("PET_SESSION_RESOLVE_BARRIER", barrier),
        ("PET_SESSION_RESOLVE_ROOT", resolve_root.as_os_str()),
        ("PYTHONPATH", python_path),
    ])
    .expect("failed to spawn long-lived PET server");
    client
        .configure(json!({
            "workspaceDirectories": [&fixture.workspace],
            "cacheDirectory": &fixture.cache,
        }))
        .expect("failed to configure long-lived PET server");

    let mut churn_expected = fixture.reset_inventory(10);
    churn_expected.sort_unstable();
    let initial = refresh_and_measure(&client, &fixture.workspace);
    assert_eq!(initial.inventory, churn_expected);
    max_ambient_environment_count =
        max_ambient_environment_count.max(initial.ambient_environment_count);
    max_ambient_manager_count = max_ambient_manager_count.max(initial.ambient_manager_count);

    let removed_prefix = fixture.workspace.join("env-0000");
    let removed_index = churn_expected
        .iter()
        .position(|identity| identity.prefix == norm_case(&removed_prefix))
        .expect("missing identity selected for replacement");
    fs::remove_dir_all(&removed_prefix).expect("failed to delete churn environment");
    churn_expected.remove(removed_index);
    churn_expected.push(fixture.create_fake_environment("replacement", "3.12.1"));
    churn_expected.sort_unstable();
    let replaced = refresh_and_measure(&client, &fixture.workspace);
    assert_eq!(replaced.inventory.len(), initial.inventory.len());
    assert_ne!(
        &replaced.inventory, &initial.inventory,
        "same-count replacement was not detected"
    );
    assert_eq!(replaced.inventory, churn_expected);

    let edited_prefix = fixture.workspace.join("env-0001");
    fs::write(
        edited_prefix.join("pyvenv.cfg"),
        "version = 3.13.2\nprompt = edited\n",
    )
    .expect("failed to edit churn environment");
    write_version_header(&edited_prefix, "3.13.2");
    let previous_index = churn_expected
        .iter()
        .position(|identity| identity.prefix == norm_case(&edited_prefix))
        .expect("missing identity selected for edit");
    let previous = churn_expected.remove(previous_index);
    churn_expected.push(EnvironmentIdentity {
        executable: previous.executable,
        prefix: previous.prefix,
        kind: previous.kind,
        name: Some("edited".to_string()),
        version: Some("3.13.2".to_string()),
    });
    churn_expected.sort_unstable();
    let edited = refresh_and_measure(&client, &fixture.workspace);
    assert_eq!(edited.inventory, churn_expected);

    let alias_prefix = fixture.workspace.join("env-0002");
    let original_executable = python_executable(&bin_directory(&alias_prefix), false);
    let alias_executable = python_executable(&bin_directory(&alias_prefix), true);
    fs::hard_link(&original_executable, &alias_executable)
        .expect("failed to create executable alias");
    fs::remove_file(&original_executable).expect("failed to remove original executable alias");
    let previous_index = churn_expected
        .iter()
        .position(|identity| identity.prefix == norm_case(&alias_prefix))
        .expect("missing identity selected for alias churn");
    let previous = churn_expected.remove(previous_index);
    churn_expected.push(EnvironmentIdentity {
        executable: norm_case(alias_executable),
        #[cfg(windows)]
        version: None,
        ..previous
    });
    churn_expected.sort_unstable();
    let aliased = refresh_and_measure(&client, &fixture.workspace);
    assert_eq!(aliased.inventory, churn_expected);

    fixture.clear_barrier();
    let resolve_executables =
        fixture.create_resolve_environments(1 + resolve_concurrency * (1 + cold_resolve_batches));
    let mut resolve_executables = resolve_executables.into_iter();
    let warmup_executable = resolve_executables
        .next()
        .expect("resolve fixture omitted warm-up interpreter");
    let mut warmup_release = BarrierReleaseGuard::new(&fixture.barrier);
    let warmup = submit_fixture_resolve(&client, &warmup_executable);
    fixture.wait_for_entered(1);
    warmup_release.release();
    warmup.wait("warm-up resolve");
    assert_eq!(fixture.released_count(), 1);
    assert_eq!(fixture.failed_count(), 0);

    fixture.clear_barrier();
    let pre_resolve_resources = observe_resources(client.process_id());
    let original_cache_before_overlap =
        cache_contents(&fixture.cache).expect("failed to capture pre-overlap cache contents");
    assert!(
        !original_cache_before_overlap.is_empty(),
        "warm-up resolve did not populate the original process cache"
    );
    let overlap_workspace = fixture._root.path().join("overlap-workspace");
    let mut overlap_expected = fixture.reset_inventory_at(&overlap_workspace, 3);
    overlap_expected.sort_unstable();
    let overlap_executables = resolve_executables
        .by_ref()
        .take(resolve_concurrency)
        .collect::<Vec<_>>();
    assert_eq!(overlap_executables.len(), resolve_concurrency);
    let mut overlap_release = BarrierReleaseGuard::new(&fixture.barrier);
    let overlap_pending = overlap_executables
        .iter()
        .map(|expected| submit_fixture_resolve(&client, expected))
        .collect::<Vec<_>>();
    fixture.wait_for_entered(resolve_concurrency);
    assert_eq!(
        fixture.entered_count(),
        resolve_concurrency,
        "every distinct resolve must start before any response is awaited"
    );
    let barrier_observed_resources = observe_resources(client.process_id());
    let info = client
        .info()
        .expect("info request was not responsive while resolves were blocked");
    assert!(
        !info.pet_version.is_empty(),
        "info returned an empty PET version"
    );
    client
        .configure(json!({
            "workspaceDirectories": [&overlap_workspace],
            "cacheDirectory": &fixture.cache,
        }))
        .expect("configure was not responsive while resolves were blocked");
    client.clear_notifications();
    if let Err(error) = client.refresh(None) {
        panic!(
            "refresh failed while cold resolves were held: {error}; entered={}, released={}, barrierReleased={}, stderr={}",
            fixture.entered_count(),
            fixture.released_count(),
            fixture.barrier.join("release").exists(),
            client.stderr_output()
        );
    }
    let (configured_overlap_inventory, overlap_ambient_environment_count) =
        fixture_inventory(client.environment_notifications(), &overlap_workspace);
    assert_eq!(
        configured_overlap_inventory, overlap_expected,
        "overlap refresh did not report the newly configured inventory"
    );
    let (fixture_manager_count, overlap_ambient_manager_count) =
        manager_counts(&client, &overlap_workspace);
    assert_eq!(
        fixture_manager_count, 0,
        "overlap fixture unexpectedly reported an environment manager"
    );
    max_ambient_environment_count =
        max_ambient_environment_count.max(overlap_ambient_environment_count);
    max_ambient_manager_count = max_ambient_manager_count.max(overlap_ambient_manager_count);
    let cache_after_overlap =
        cache_contents(&fixture.cache).expect("failed to capture post-overlap cache contents");
    assert!(
        original_cache_entries_unchanged(&original_cache_before_overlap, &cache_after_overlap),
        "overlap reconfiguration modified or deleted an original cache entry"
    );
    assert_eq!(
        fixture.released_count(),
        0,
        "a cold resolve crossed the fixture barrier before its release"
    );
    assert!(
        !fixture.barrier.join("release").exists(),
        "the resolve barrier was released before responsiveness checks completed"
    );
    let release_at = Instant::now();
    assert!(
        overlap_pending
            .iter()
            .all(|request| request.submitted_at() < release_at),
        "all resolve requests must be submitted before releasing the fixture barrier"
    );
    overlap_release.release();
    for request in overlap_pending {
        request.wait("barrier-proven concurrent resolve");
    }
    assert_eq!(
        fixture.released_count(),
        resolve_concurrency,
        "every overlap interpreter must confirm barrier release"
    );
    assert_eq!(
        fixture.failed_count(),
        0,
        "an overlap interpreter timed out at the barrier"
    );
    let post_overlap_resources = observe_resources(client.process_id());

    let mut resolve_latency_us = Vec::new();
    let mut resolve_batch_resources: Vec<(usize, ResourceSample)> = Vec::new();
    for batch in 0..cold_resolve_batches {
        fixture.clear_barrier();
        let mut latency_release = BarrierReleaseGuard::new(&fixture.barrier);
        latency_release.release();
        let latency_executables = resolve_executables
            .by_ref()
            .take(resolve_concurrency)
            .collect::<Vec<_>>();
        assert_eq!(latency_executables.len(), resolve_concurrency);
        let latency_pending = latency_executables
            .iter()
            .map(|expected| submit_fixture_resolve(&client, expected))
            .collect::<Vec<_>>();
        resolve_latency_us.extend(latency_pending.into_iter().map(|request| {
            let (_, latency) = request.wait("concurrent resolve");
            latency.as_micros()
        }));
        assert_eq!(
            fixture.entered_count(),
            resolve_concurrency,
            "latency batch must cold-start every distinct interpreter"
        );
        assert_eq!(
            fixture.released_count(),
            resolve_concurrency,
            "latency batch interpreters must confirm barrier release"
        );
        assert_eq!(
            fixture.failed_count(),
            0,
            "a latency batch interpreter timed out at the barrier"
        );
        resolve_batch_resources.push((batch, observe_resources(client.process_id())));
    }
    assert!(
        resolve_executables.next().is_none(),
        "resolve fixture count did not match the exercised cold batches"
    );

    let resource_after = observe_resources(client.process_id());
    if let (Some(baseline), Some(after)) = (post_overlap_resources.threads, resource_after.threads)
    {
        assert!(
            after <= baseline,
            "PET worker threads grew across repeated cold batches: post-overlap observation {baseline}, post-workload {after}"
        );
    }
    if let (Some(baseline), Some(after)) = (
        post_overlap_resources.descriptors,
        resource_after.descriptors,
    ) {
        assert!(
            after <= baseline,
            "PET handles/descriptors grew across repeated cold batches: post-overlap observation {baseline}, post-workload {after}"
        );
    }

    let mut all_resource_samples = vec![
        pre_resolve_resources.clone(),
        barrier_observed_resources.clone(),
        post_overlap_resources.clone(),
        resource_after.clone(),
    ];
    all_resource_samples.extend(
        inventory_resource_samples
            .iter()
            .map(|(_, sample)| sample.clone()),
    );
    all_resource_samples.extend(
        resolve_batch_resources
            .iter()
            .map(|(_, sample)| sample.clone()),
    );
    let observed_peak = observed_resource_peak(&all_resource_samples);
    shutdown_client(&client, "long-lived benchmark server");

    let first_process_sample_count = sizes.len();
    let new_process_sample_count = sizes.len();
    let same_process_warm_sample_count = sizes.len() * samples_per_size;
    println!(
        "SESSION_METRICS {}",
        serde_json::to_string(&json!({
            "status": "passed",
            "mode": if stress { "stress" } else { "fast" },
            "sizes": sizes,
            "samplesPerSize": samples_per_size,
            "measurementCounts": {
                "firstProcessEmptyDiskCache": first_process_sample_count,
                "newProcessAfterFirstRefresh": new_process_sample_count,
                "sameProcessWarm": same_process_warm_sample_count,
                "persistentCacheResolve": persistent_cache_resolve_samples.len(),
                "cacheUsage": cache_usage.len(),
                "inventoryResources": inventory_resource_samples.len(),
                "resolveLatency": resolve_latency_us.len(),
                "resolveBatchResources": resolve_batch_resources.len(),
            },
            "refreshScenarioSamples": refresh_scenario_samples,
            "cacheUsage": cache_usage,
            "persistentCacheResolveSamples": persistent_cache_resolve_samples,
            "persistentCacheAfterColdResolve": cache_json(cache_after_cold_resolve),
            "resolveConcurrency": resolve_concurrency,
            "coldResolveBatches": cold_resolve_batches,
            "resolveLatencyUs": resolve_latency_us,
            "overlapProcessesStarted": resolve_concurrency,
            "overlapAmbientEnvironmentCount": overlap_ambient_environment_count,
            "overlapAmbientManagerCount": overlap_ambient_manager_count,
            "maxAmbientEnvironmentCount": max_ambient_environment_count,
            "maxAmbientManagerCount": max_ambient_manager_count,
            "latencyProcessesStarted": resolve_concurrency * cold_resolve_batches,
            "inventoryResourceSamples": inventory_resource_samples
                .iter()
                .map(|(inventory_size, sample)| json!({
                    "inventorySize": inventory_size,
                    "resources": resource_json(sample),
                }))
                .collect::<Vec<_>>(),
            "resolveBatchResourceSamples": resolve_batch_resources
                .iter()
                .map(|(batch, sample)| json!({
                    "batch": batch,
                    "resources": resource_json(sample),
                }))
                .collect::<Vec<_>>(),
            "preResolveResources": resource_json(&pre_resolve_resources),
            "barrierObservedResources": resource_json(&barrier_observed_resources),
            "postOverlapResources": resource_json(&post_overlap_resources),
            "observedResourcePeak": resource_json(&observed_peak),
            "resourceAfter": resource_json(&resource_after),
            "rssDeltaFromPreResolveBytes": i128::from(resource_after.resident_bytes)
                - i128::from(pre_resolve_resources.resident_bytes),
        }))
        .expect("failed to serialize session metrics")
    );
}
