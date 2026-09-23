// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_fs::path::norm_case;
use serde_json::{json, Value};
use std::fs;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use tempfile::TempDir;

mod jsonrpc_client;

use jsonrpc_client::{EnvironmentNotification, PetJsonRpcClient};

struct RawRpcClient {
    child: Child,
    responses: mpsc::Receiver<std::io::Result<Value>>,
    reader: Option<JoinHandle<()>>,
}

impl RawRpcClient {
    fn spawn() -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pet"));
        command
            .arg("server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env_clear()
            .env("PATH", "");
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SYSTEMROOT") {
            command.env("SYSTEMROOT", system_root);
        }
        let mut child = command.spawn().expect("raw fixture must spawn PET");
        let stdout = child.stdout.take().expect("PET stdout must be piped");
        let (sender, responses) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                match jsonrpc_client::read_message(&mut stdout) {
                    Ok(Some(message)) => {
                        if sender.send(Ok(message)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => return,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        return;
                    }
                }
            }
        });
        Self {
            child,
            responses,
            reader: Some(reader),
        }
    }

    fn send(&mut self, message: Value) {
        let body = serde_json::to_vec(&message).unwrap();
        let stdin = self.child.stdin.as_mut().expect("PET stdin must be piped");
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        stdin.write_all(&body).unwrap();
        stdin.flush().unwrap();
    }

    fn receive(&self) -> Value {
        self.responses
            .recv_timeout(Duration::from_secs(10))
            .expect("PET must respond within ten seconds")
            .expect("PET must emit valid JSONRPC")
    }
}

impl Drop for RawRpcClient {
    fn drop(&mut self) {
        // EOF shutdown is tracked separately; kill only this fixture's child before closing stdin.
        if let Err(error) = self.child.kill() {
            eprintln!("Failed to stop raw RPC fixture: {error}");
        }
        if let Err(error) = self.child.wait() {
            eprintln!("Failed to reap raw RPC fixture: {error}");
        }
        self.child.stdin.take();
        if let Some(reader) = self.reader.take() {
            if reader.join().is_err() {
                eprintln!("Raw RPC fixture reader panicked");
            }
        }
    }
}

#[test]
fn request_ids_round_trip_through_success_and_error_responses() {
    let mut client = RawRpcClient::spawn();
    for id in [
        json!("request-1"),
        json!("\u{03c0}-request"),
        json!(""),
        json!(0),
        json!(u64::from(u32::MAX) + 1),
        json!(u64::MAX),
        json!(i64::MIN),
        json!(-7),
        json!(1.5),
        Value::Null,
    ] {
        for (method, params, error_code) in [
            ("info", json!({}), None),
            ("unknown", json!({}), Some(-1)),
            ("resolve", json!({"executable": 42}), Some(-4)),
        ] {
            client.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
            let response = client.receive();
            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response.get("id"), Some(&id));
            if let Some(code) = error_code {
                assert_eq!(response["error"]["code"], code);
                assert!(response.get("result").is_none());
            } else {
                assert!(response["result"]["petVersion"].is_string());
                assert!(response.get("error").is_none());
            }
        }
        client.send(json!({"jsonrpc": "2.0", "id": id}));
        let response = client.receive();
        assert_eq!(response.get("id"), Some(&id));
        assert_eq!(response["error"]["code"], -3);
    }
}

#[test]
fn missing_and_invalid_request_ids_have_distinct_wire_behavior() {
    let mut client = RawRpcClient::spawn();
    client.send(json!({"jsonrpc": "2.0", "method": "info", "params": {}}));
    client.send(json!({"jsonrpc": "2.0", "id": "sentinel", "method": "info", "params": {}}));
    assert_eq!(client.receive()["id"], "sentinel");
    for id in [json!(true), json!(false), json!([]), json!({"id": 7})] {
        client.send(json!({"jsonrpc": "2.0", "id": id, "method": "info", "params": {}}));
        let response = client.receive();
        assert_eq!(response.get("id"), Some(&Value::Null));
        assert_eq!(response["error"]["code"], -32600);
        assert!(response.get("result").is_none());
    }
    client.send(json!({"jsonrpc": "2.0", "id": "after-invalid", "method": "info", "params": {}}));
    assert_eq!(client.receive()["id"], "after-invalid");
}

fn create_fake_workspace(prompt: &str) -> (TempDir, PathBuf, PathBuf) {
    let temp_dir = tempfile::tempdir().expect("failed to create temp directory");
    let workspace = temp_dir.path().join("workspace");
    let venv = workspace.join(".venv");

    #[cfg(windows)]
    let bin_dir = venv.join("Scripts");
    #[cfg(unix)]
    let bin_dir = venv.join("bin");

    fs::create_dir_all(&bin_dir).expect("failed to create fake venv directories");
    fs::write(
        venv.join("pyvenv.cfg"),
        format!("version = 3.11.0\nprompt = {prompt}\n"),
    )
    .expect("failed to write pyvenv.cfg");
    fs::write(python_executable_path(&bin_dir), "fake python")
        .expect("failed to create fake python executable");

    (temp_dir, workspace, venv)
}

fn create_fake_workspace_with_projects(
    prompt_prefix: &str,
    project_count: usize,
) -> (TempDir, PathBuf, Vec<PathBuf>) {
    let temp_dir = tempfile::tempdir().expect("failed to create temp directory");
    let workspace = temp_dir.path().join("workspace");
    let mut venvs = Vec::new();

    for index in 0..project_count {
        let venv = workspace.join(format!("env-{index}"));
        #[cfg(windows)]
        let bin_dir = venv.join("Scripts");
        #[cfg(unix)]
        let bin_dir = venv.join("bin");

        fs::create_dir_all(&bin_dir).expect("failed to create fake venv directories");
        fs::write(
            venv.join("pyvenv.cfg"),
            format!("version = 3.11.0\nprompt = {prompt_prefix}-{index}\n"),
        )
        .expect("failed to write pyvenv.cfg");
        fs::write(python_executable_path(&bin_dir), "fake python")
            .expect("failed to create fake python executable");
        venvs.push(venv);
    }

    (temp_dir, workspace, venvs)
}

fn python_executable_path(bin_dir: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        bin_dir.join("python.exe")
    }

    #[cfg(unix)]
    {
        bin_dir.join("python")
    }
}

fn cache_dir(root: &TempDir) -> PathBuf {
    root.path().join("cache")
}

fn normalized_notification_path(path: &Option<String>) -> Option<PathBuf> {
    path.as_ref().map(|path| norm_case(PathBuf::from(path)))
}

fn assert_single_environment(
    environments: &[EnvironmentNotification],
    expected_executable: &Path,
    expected_prefix: &Path,
    expected_name: &str,
    stderr: &str,
) {
    assert_eq!(
        environments.len(),
        1,
        "expected exactly one environment notification, got {environments:?}; stderr: {stderr}"
    );
    let environment = &environments[0];
    assert_eq!(environment.kind.as_deref(), Some("Venv"));
    assert_eq!(environment.name.as_deref(), Some(expected_name));
    assert_eq!(
        normalized_notification_path(&environment.executable).as_deref(),
        Some(norm_case(expected_executable)).as_deref()
    );
    assert_eq!(
        normalized_notification_path(&environment.prefix).as_deref(),
        Some(norm_case(expected_prefix)).as_deref()
    );
    assert_eq!(environment.error, None);
}

#[test]
fn info_reports_pet_version_and_optional_build_metadata() {
    let client = PetJsonRpcClient::spawn().expect("failed to spawn PET server");

    let info = client.info().expect("info request failed");

    assert_eq!(info.pet_version, env!("CARGO_PKG_VERSION"));
    // build_id / commit_sha are populated from env vars set at compile time by CI.
    // For local dev builds they will be None; assert non-empty only when present.
    assert!(info
        .build_id
        .as_deref()
        .is_none_or(|build_id| !build_id.is_empty()));
    assert!(info
        .commit_sha
        .as_deref()
        .is_none_or(|commit_sha| !commit_sha.is_empty()));
}

#[test]
fn configure_and_workspace_refresh_report_fake_venv() {
    let client = PetJsonRpcClient::spawn().expect("failed to spawn PET server");
    let (temp_dir, workspace, venv) = create_fake_workspace("workspace-env");

    client
        .configure(json!({
            "workspaceDirectories": [workspace.clone()],
            "cacheDirectory": cache_dir(&temp_dir),
        }))
        .expect("configure request failed");

    client.clear_notifications();
    let refresh = client
        .refresh(Some(json!({ "searchPaths": [workspace.clone()] })))
        .expect("refresh request failed");

    client
        .wait_for_telemetry_event_count("RefreshPerformance", 1, Duration::from_secs(5))
        .expect("timed out waiting for refresh performance telemetry");
    let progress = client.telemetry_events("RefreshProgress");
    assert_eq!(
        progress.len(),
        8,
        "expected started/completed for four phases"
    );
    assert!(progress.iter().all(|event| {
        event["data"]["refreshProgress"]["refreshId"].as_u64() == Some(refresh.refresh_id)
    }));
    assert!(progress.iter().all(|event| {
        let data = &event["data"]["refreshProgress"];
        data.get("executable").is_none()
            && data.get("prefix").is_none()
            && data.get("path").is_none()
    }));
    let environments = client.environment_notifications();
    assert_single_environment(
        &environments,
        &python_executable_path(&venv.join(if cfg!(windows) { "Scripts" } else { "bin" })),
        &venv,
        "workspace-env",
        &client.stderr_output(),
    );
    assert_eq!(
        client.manager_notifications().len(),
        0,
        "fake venv refresh should not report any managers"
    );
    assert_eq!(client.telemetry_event_count("RefreshPerformance"), 1);
}

#[test]
fn concurrent_identical_refresh_requests_share_one_notification_stream() {
    let client = PetJsonRpcClient::spawn().expect("failed to spawn PET server");
    let expected_environment_count = 24;
    let (temp_dir, workspace, venvs) =
        create_fake_workspace_with_projects("shared-env", expected_environment_count);

    client
        .configure(json!({
            "workspaceDirectories": [workspace.clone()],
            "cacheDirectory": cache_dir(&temp_dir),
        }))
        .expect("configure request failed");

    client.clear_notifications();
    let request_params = json!({ "searchPaths": [workspace.clone()] });

    let mut handles = Vec::new();
    for _ in 0..3 {
        let client = client.clone();
        let params = request_params.clone();
        handles.push(thread::spawn(move || client.refresh(Some(params))));
    }

    let refresh_results = handles
        .into_iter()
        .map(|handle| handle.join().expect("refresh thread panicked"))
        .collect::<Result<Vec<_>, _>>()
        .expect("concurrent refresh request failed");

    assert_eq!(refresh_results.len(), 3);
    for result in refresh_results.windows(2) {
        assert_eq!(
            result[0], result[1],
            "joined refreshes should reuse the same refresh result and ID"
        );
    }

    client
        .wait_for_notification_count(
            "environment",
            expected_environment_count,
            Duration::from_secs(5),
        )
        .expect("timed out waiting for environment notifications");
    client
        .wait_for_telemetry_event_count("RefreshPerformance", 1, Duration::from_secs(5))
        .expect("timed out waiting for refresh performance telemetry");
    let progress = client.telemetry_events("RefreshProgress");
    assert_eq!(progress.len(), 8);
    assert!(progress.iter().all(|event| {
        event["data"]["refreshProgress"]["refreshId"].as_u64()
            == Some(refresh_results[0].refresh_id)
    }));

    let environments = client.environment_notifications();
    assert_eq!(
        environments.len(),
        expected_environment_count,
        "expected one environment notification per fake venv; stderr: {}",
        client.stderr_output()
    );
    let mut names = environments
        .iter()
        .map(|environment| environment.name.clone().unwrap_or_default())
        .collect::<Vec<String>>();
    names.sort();
    let mut expected_names = (0..expected_environment_count)
        .map(|index| format!("shared-env-{index}"))
        .collect::<Vec<String>>();
    expected_names.sort();
    assert_eq!(names, expected_names);
    for venv in venvs {
        let expected_executable = norm_case(python_executable_path(&venv.join(if cfg!(windows) {
            "Scripts"
        } else {
            "bin"
        })));
        assert!(
            environments.iter().any(|environment| {
                normalized_notification_path(&environment.executable).as_deref()
                    == Some(expected_executable.as_path())
            }),
            "expected to find notification for {:?}; notifications: {:?}; stderr: {}",
            expected_executable,
            environments,
            client.stderr_output()
        );
    }
    assert_eq!(
        client.notification_count("environment"),
        expected_environment_count,
        "identical refresh requests should emit one environment notification stream"
    );
    assert_eq!(
        client.telemetry_event_count("RefreshPerformance"),
        1,
        "identical refresh requests should emit one performance event"
    );
}

#[test]
fn concurrent_distinct_refresh_requests_run_separately() {
    let client = PetJsonRpcClient::spawn().expect("failed to spawn PET server");
    let (temp_dir_a, workspace_a, venv_a) = create_fake_workspace("first-env");
    let (temp_dir_b, workspace_b, venv_b) = create_fake_workspace("second-env");

    client
        .configure(json!({
            "workspaceDirectories": [workspace_a.clone(), workspace_b.clone()],
            "cacheDirectory": cache_dir(&temp_dir_a),
        }))
        .expect("configure request failed");

    let _temp_dir_b = temp_dir_b;
    client.clear_notifications();

    let client_a = client.clone();
    let client_b = client.clone();
    let handle_a =
        thread::spawn(move || client_a.refresh(Some(json!({ "searchPaths": [workspace_a] }))));
    let handle_b =
        thread::spawn(move || client_b.refresh(Some(json!({ "searchPaths": [workspace_b] }))));

    let result_a = handle_a
        .join()
        .expect("first refresh thread panicked")
        .expect("first refresh failed");
    let result_b = handle_b
        .join()
        .expect("second refresh thread panicked")
        .expect("second refresh failed");
    assert_ne!(result_a.refresh_id, result_b.refresh_id);

    client
        .wait_for_notification_count("environment", 2, Duration::from_secs(5))
        .expect("timed out waiting for environment notifications");
    client
        .wait_for_telemetry_event_count("RefreshPerformance", 2, Duration::from_secs(5))
        .expect("timed out waiting for refresh performance telemetry");
    let mut environments = client.environment_notifications();
    environments.sort_by(|left, right| left.name.cmp(&right.name));

    assert_eq!(
        environments.len(),
        2,
        "distinct refreshes should each report their targeted workspace envs; stderr: {}",
        client.stderr_output()
    );

    assert_eq!(environments[0].kind.as_deref(), Some("Venv"));
    assert_eq!(environments[0].name.as_deref(), Some("first-env"));
    assert_eq!(
        normalized_notification_path(&environments[0].executable).as_deref(),
        Some(
            norm_case(python_executable_path(&venv_a.join(if cfg!(windows) {
                "Scripts"
            } else {
                "bin"
            }),))
            .as_path()
        )
    );
    assert_eq!(environments[1].kind.as_deref(), Some("Venv"));
    assert_eq!(environments[1].name.as_deref(), Some("second-env"));
    assert_eq!(
        normalized_notification_path(&environments[1].executable).as_deref(),
        Some(
            norm_case(python_executable_path(&venv_b.join(if cfg!(windows) {
                "Scripts"
            } else {
                "bin"
            }),))
            .as_path()
        )
    );
    assert_eq!(
        client.telemetry_event_count("RefreshPerformance"),
        2,
        "distinct refresh requests should emit separate performance events"
    );
}
