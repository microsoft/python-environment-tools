// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_fs::path::norm_case;
use serde_json::{json, Value};
use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tempfile::TempDir;

mod jsonrpc_client;

use jsonrpc_client::{EnvironmentNotification, PetJsonRpcClient};

fn frame_with_headers(payload: &[u8], headers: &[(&str, &str)], line_ending: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    for (name, value) in headers {
        frame.extend_from_slice(name.as_bytes());
        frame.extend_from_slice(b": ");
        frame.extend_from_slice(value.as_bytes());
        frame.extend_from_slice(line_ending);
    }
    frame.extend_from_slice(line_ending);
    frame.extend_from_slice(payload);
    frame
}

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
            .stderr(Stdio::inherit());
        jsonrpc_client::configure_isolated_pet_environment(&mut command);
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
        self.send_payload(&body);
    }

    fn send_payload(&mut self, body: &[u8]) {
        let content_length = body.len().to_string();
        let frame = frame_with_headers(
            body,
            &[("Content-Length", content_length.as_str())],
            b"\r\n",
        );
        self.write_raw(&frame);
    }

    fn write_raw(&mut self, bytes: &[u8]) {
        let stdin = self.child.stdin.as_mut().expect("PET stdin must be piped");
        stdin.write_all(bytes).unwrap();
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
        self.child.stdin.take();
        if let Err(error) =
            jsonrpc_client::shutdown_fixture(&mut self.child, Duration::from_secs(4))
        {
            eprintln!("Failed to stop raw RPC fixture: {error}");
            return;
        }
        if let Some(reader) = self.reader.take() {
            if let Err(error) = jsonrpc_client::join_reader(reader, Duration::from_secs(4)) {
                eprintln!("Failed to finish raw RPC fixture reader: {error}");
            }
        }
    }
}

fn assert_rpc_error(response: &Value, expected_id: &Value, expected_code: i64) {
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response.get("id"), Some(expected_id));
    assert_eq!(response["error"]["code"], expected_code);
    assert!(response.get("result").is_none());
}

#[test]
fn native_wire_accepts_header_variants_fragmentation_and_consecutive_frames() {
    let mut client = RawRpcClient::spawn();
    let crlf_payload = br#"{"jsonrpc":"2.0","id":"crlf-content-type-first","method":"info"}"#;
    let lf_payload =
        "{\"jsonrpc\":\"2.0\",\"id\":\"lf-snowman-\u{2603}\",\"method\":\"info\"}".as_bytes();
    let no_content_type_payload = br#"{"jsonrpc":"2.0","id":"no-content-type","method":"info"}"#;
    assert!(
        lf_payload.iter().any(|byte| !byte.is_ascii()),
        "fixture must exercise byte lengths rather than character counts"
    );

    let crlf_length = crlf_payload.len().to_string();
    let lf_length = lf_payload.len().to_string();
    let no_content_type_length = no_content_type_payload.len().to_string();
    let mut wire = frame_with_headers(
        crlf_payload,
        &[
            ("cOnTeNt-TyPe", "application/vscode-jsonrpc; charset=utf-8"),
            ("X-Before-Length", "accepted"),
            ("cOnTeNt-LeNgTh", crlf_length.as_str()),
        ],
        b"\r\n",
    );
    wire.extend(frame_with_headers(
        lf_payload,
        &[
            ("CONTENT-LENGTH", lf_length.as_str()),
            ("x-after-length", "accepted"),
            ("CONTENT-TYPE", "application/vscode-jsonrpc; charset=utf-8"),
        ],
        b"\n",
    ));
    wire.extend(frame_with_headers(
        no_content_type_payload,
        &[
            ("X-Optional-Content-Type", "omitted"),
            ("Content-Length", no_content_type_length.as_str()),
        ],
        b"\r\n",
    ));

    for fragment in wire.chunks(3) {
        client.write_raw(fragment);
    }

    for expected_id in [
        "crlf-content-type-first",
        "lf-snowman-\u{2603}",
        "no-content-type",
    ] {
        let response = client.receive();
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], expected_id);
        assert!(response["result"]["petVersion"].is_string());
        assert!(response.get("error").is_none());
    }
}

#[test]
fn complete_invalid_json_and_utf8_frames_recover_for_the_next_frame() {
    let mut client = RawRpcClient::spawn();

    client.send_payload(br#"{"jsonrpc":"2.0","id":"malformed","method":"info""#);
    assert_rpc_error(&client.receive(), &Value::Null, -32700);

    client.send_payload(&[b'{', b'"', 0xff, b'"', b':', b'1', b'}']);
    assert_rpc_error(&client.receive(), &Value::Null, -32700);

    client.send(json!({
        "jsonrpc": "2.0",
        "id": "after-parse-errors",
        "method": "info"
    }));
    let response = client.receive();
    assert_eq!(response["id"], "after-parse-errors");
    assert!(response["result"]["petVersion"].is_string());
}

#[test]
fn native_wire_validates_envelopes_params_and_legacy_errors() {
    let mut client = RawRpcClient::spawn();

    for invalid in [
        Value::Null,
        json!(false),
        json!(42),
        json!("request"),
        json!([]),
        json!([{"jsonrpc": "2.0", "id": "batch", "method": "info"}]),
    ] {
        client.send(invalid);
        assert_rpc_error(&client.receive(), &Value::Null, -32600);
    }

    for request in [
        json!({"id": "missing-version", "method": "info"}),
        json!({"jsonrpc": "1.0", "id": "wrong-version", "method": "info"}),
        json!({"jsonrpc": 2.0, "id": 17, "method": "info"}),
    ] {
        let expected_id = request["id"].clone();
        client.send(request);
        assert_rpc_error(&client.receive(), &expected_id, -32600);
    }

    for invalid_id in [json!([]), json!({"nested": "id"})] {
        client.send(json!({
            "jsonrpc": "2.0",
            "id": invalid_id,
            "method": "info"
        }));
        assert_rpc_error(&client.receive(), &Value::Null, -32600);
    }

    for (index, params) in [json!(false), json!(7), json!("scalar")]
        .into_iter()
        .enumerate()
    {
        let id = json!(format!("invalid-params-{index}"));
        client.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "info",
            "params": params
        }));
        assert_rpc_error(&client.receive(), &id, -32602);
    }

    for (id, params) in [
        ("missing-params", None),
        ("null-params", Some(Value::Null)),
        ("array-params", Some(json!([]))),
        ("object-params", Some(json!({}))),
    ] {
        let mut request = json!({"jsonrpc": "2.0", "id": id, "method": "info"});
        if let Some(params) = params {
            request["params"] = params;
        }
        client.send(request);
        let response = client.receive();
        assert_eq!(response["id"], id);
        assert!(response["result"]["petVersion"].is_string());
    }

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "info",
        "params": "invalid-notification-params"
    }));
    client.send(json!({
        "jsonrpc": "2.0",
        "id": "notification-sentinel",
        "method": "info"
    }));
    let response = client.receive();
    assert_eq!(response["id"], "notification-sentinel");
    assert!(response["result"]["petVersion"].is_string());

    client.send(json!({"jsonrpc": "2.0", "id": "missing-method"}));
    assert_rpc_error(&client.receive(), &json!("missing-method"), -3);
    client.send(json!({
        "jsonrpc": "2.0",
        "id": "unknown-method",
        "method": "unknown"
    }));
    assert_rpc_error(&client.receive(), &json!("unknown-method"), -1);
    client.send(json!({
        "jsonrpc": "2.0",
        "id": "handler-invalid-param",
        "method": "resolve",
        "params": {"executable": 42}
    }));
    assert_rpc_error(&client.receive(), &json!("handler-invalid-param"), -4);
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

struct ShutdownFixture {
    child: Child,
}

impl ShutdownFixture {
    fn spawn() -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pet"));
        command
            .arg("server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        jsonrpc_client::configure_isolated_pet_environment(&mut command);
        Self {
            child: command.spawn().expect("shutdown fixture must spawn PET"),
        }
    }

    fn send(&mut self, body: &[u8]) {
        let stdin = self.child.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        stdin.write_all(body).unwrap();
        stdin.flush().unwrap();
    }

    fn write_raw(&mut self, bytes: &[u8]) {
        let stdin = self.child.stdin.as_mut().unwrap();
        stdin.write_all(bytes).unwrap();
        stdin.flush().unwrap();
    }
}

impl Drop for ShutdownFixture {
    fn drop(&mut self) {
        self.child.stdin.take();
        if let Err(error) =
            jsonrpc_client::shutdown_fixture(&mut self.child, Duration::from_secs(4))
        {
            eprintln!("Failed to stop shutdown fixture: {error}");
        }
    }
}

fn assert_fatal_framing_input(name: &str, input: &[u8]) {
    let mut fixture = ShutdownFixture::spawn();
    fixture.write_raw(input);
    let started = Instant::now();
    fixture.child.stdin.take();
    let status = jsonrpc_client::wait_for_exit(&mut fixture.child, Duration::from_secs(1))
        .unwrap_or_else(|error| panic!("{name} did not terminate within one second: {error}"));
    assert!(
        !status.success(),
        "{name} must terminate the server unsuccessfully"
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{name} exceeded the shutdown budget"
    );
    let mut stderr = Vec::new();
    fixture
        .child
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    assert!(!stderr.is_empty(), "{name} must be reported");
    assert!(
        stderr.len() < 4096,
        "{name} produced an error flood of {} bytes",
        stderr.len()
    );
}

#[test]
fn invalid_and_oversize_framing_terminates_with_bounded_diagnostics() {
    const MAX_HEADER_BYTES: usize = 8 * 1024;
    const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

    let mut oversized_header = b"X-Oversized: ".to_vec();
    oversized_header.resize(MAX_HEADER_BYTES + 1, b'x');
    oversized_header.extend_from_slice(b"\r\n\r\n");

    let cases = [
        (
            "invalid Content-Length",
            b"Content-Length: twelve\r\n\r\n".to_vec(),
        ),
        (
            "missing Content-Length",
            b"Content-Type: application/json\r\n\r\n".to_vec(),
        ),
        (
            "duplicate Content-Length",
            b"Content-Length: 0\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "overflowing Content-Length",
            b"Content-Length: 184467440737095516160\r\n\r\n".to_vec(),
        ),
        (
            "oversize Content-Length",
            format!("Content-Length: {}\r\n\r\n", MAX_PAYLOAD_BYTES + 1).into_bytes(),
        ),
        ("malformed header", b"Not-A-Header\r\n\r\n".to_vec()),
        ("oversize header", oversized_header),
    ];

    for (name, input) in cases {
        assert_fatal_framing_input(name, &input);
    }
}

#[test]
fn stdin_eof_after_exchange_exits_cleanly_within_one_second() {
    let client = PetJsonRpcClient::spawn().unwrap();
    client.info().unwrap();
    let started = Instant::now();
    let status = client.shutdown(Duration::from_secs(1)).unwrap();
    assert!(status.success(), "normal EOF shutdown failed: {status}");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(
        client.stderr_output().len() < 4096,
        "EOF must not produce an error flood"
    );
}

#[test]
fn truncated_input_exits_unsuccessfully_without_an_error_flood() {
    for bytes in [
        b"Content-Length: 2".as_slice(),
        b"Content-Length: 2\r\n\r\n{".as_slice(),
    ] {
        let mut fixture = ShutdownFixture::spawn();
        fixture
            .child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(bytes)
            .unwrap();
        let started = Instant::now();
        fixture.child.stdin.take();
        let status =
            jsonrpc_client::wait_for_exit(&mut fixture.child, Duration::from_secs(1)).unwrap();
        assert!(!status.success());
        assert!(started.elapsed() < Duration::from_secs(1));
        let mut stderr = Vec::new();
        fixture
            .child
            .stderr
            .take()
            .unwrap()
            .read_to_end(&mut stderr)
            .unwrap();
        assert!(!stderr.is_empty());
        assert!(stderr.len() < 4096, "truncated input must be reported once");
    }
}

#[test]
fn closed_output_exits_without_waiting_for_stdin_eof() {
    // Concurrent fork/exec can temporarily inherit a pipe reader despite CLOEXEC.
    // Isolate this scenario so its dropped handle really is the final reader.
    if std::env::var_os("PET_TEST_CLOSED_OUTPUT_CHILD").is_none() {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "closed_output_exits_without_waiting_for_stdin_eof",
                "--nocapture",
            ])
            .env("PET_TEST_CLOSED_OUTPUT_CHILD", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("isolated closed-output fixture must spawn");
        // Cover readiness, both forced-shutdown waits, reader joining, and Drop cleanup.
        let status = jsonrpc_client::shutdown_fixture(&mut child, Duration::from_secs(40)).unwrap();
        assert!(
            status.success(),
            "isolated closed-output fixture failed: {status}"
        );
        return;
    }

    let mut fixture = ShutdownFixture::spawn();
    let mut stdout = BufReader::new(fixture.child.stdout.take().unwrap());
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let response = jsonrpc_client::read_message(&mut stdout);
        let _ = sender.send((stdout, response));
    });
    fixture.send(br#"{"jsonrpc":"2.0","id":"ready","method":"info"}"#);
    let (stdout, response) = match receiver.recv_timeout(Duration::from_secs(10)) {
        Ok(result) => result,
        Err(error) => {
            drop(receiver);
            fixture.child.stdin.take();
            let shutdown =
                jsonrpc_client::shutdown_fixture(&mut fixture.child, Duration::from_secs(4));
            let joined = jsonrpc_client::join_reader(reader, Duration::from_secs(4));
            panic!("server readiness failed: {error}; shutdown: {shutdown:?}; reader: {joined:?}");
        }
    };
    jsonrpc_client::join_reader(reader, Duration::from_secs(1)).unwrap();
    let response = response
        .unwrap()
        .expect("ready server must respond to info");
    assert_eq!(response["id"], "ready");
    drop(stdout);

    let started = Instant::now();
    fixture.send(br#"{"jsonrpc":"2.0","id":1,"method":"info"}"#);
    let status = jsonrpc_client::wait_for_exit(&mut fixture.child, Duration::from_secs(1)).unwrap();
    assert!(
        !status.success(),
        "broken output must produce a nonzero exit"
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(
        fixture.child.stdin.is_some(),
        "input remains open throughout this check"
    );
}

#[test]
fn stdin_eof_exits_while_output_is_not_drained() {
    let mut fixture = ShutdownFixture::spawn();
    let mut stdout = fixture.child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let mut byte = [0];
        let result = stdout.read_exact(&mut byte);
        sender
            .send((stdout, result, byte))
            .expect("fixture must wait for output to start");
    });
    let body =
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"x".repeat(128 * 1024),"method":"info"}))
            .unwrap();
    fixture.send(&body);
    let (_unread_output, result, first_byte) =
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    result.unwrap();
    assert_eq!(first_byte, [b'C']);
    let started = Instant::now();
    fixture.child.stdin.take();
    let status = jsonrpc_client::wait_for_exit(&mut fixture.child, Duration::from_secs(1)).unwrap();
    assert!(status.success());
    assert!(started.elapsed() < Duration::from_secs(1));
    reader.join().unwrap();
}

fn wait_for_descendant_lease(
    mut try_lock: impl FnMut() -> Result<(), fs::TryLockError>,
    timeout: Duration,
) -> std::io::Result<()> {
    let started = Instant::now();
    loop {
        match try_lock() {
            Ok(()) => return Ok(()),
            Err(fs::TryLockError::Error(error)) => return Err(error),
            Err(fs::TryLockError::WouldBlock) => {}
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "descendant still holds its lease at the shutdown deadline",
            ));
        }
        thread::sleep(Duration::from_millis(10).min(remaining));
    }
}

#[test]
fn descendant_lease_wait_is_bounded_and_requires_release() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("lease");
    let holder = fs::File::create(&path).unwrap();
    holder.try_lock().unwrap();
    let lease = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    let started = Instant::now();
    assert_eq!(
        wait_for_descendant_lease(|| lease.try_lock(), Duration::from_millis(20))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::TimedOut
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    let mut holder = Some(holder);
    let mut attempts = 0;
    wait_for_descendant_lease(
        || {
            attempts += 1;
            let result = lease.try_lock();
            if attempts == 1 {
                assert!(matches!(result, Err(fs::TryLockError::WouldBlock)));
                drop(holder.take());
            }
            result
        },
        Duration::from_secs(1),
    )
    .expect("lease polling must observe release after initial contention");
    assert_eq!(attempts, 2);
}

#[cfg(feature = "ci")]
#[test]
fn stdin_eof_cancels_an_active_interpreter_and_its_descendant() {
    let output = Command::new(if cfg!(windows) { "python" } else { "python3" })
        .args([
            "-S",
            "-c",
            "import sys; sys.stdout.buffer.write(sys.executable.encode('utf-8'))",
        ])
        .output()
        .expect("CI must provide Python for the active-probe fixture");
    assert!(output.status.success());
    let python = String::from_utf8(output.stdout).unwrap();
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("sitecustomize.py"),
        include_str!("fixtures/shutdown_probe.py"),
    )
    .unwrap();
    let control_path = directory.path().join("control");
    let lease_path = directory.path().join("lease");
    let ready_path = directory.path().join("ready");
    let mut control = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&control_path)
        .unwrap();
    control.write_all(b"x").unwrap();
    control.try_lock().unwrap();
    let client = PetJsonRpcClient::spawn_with_environment(&[
        ("PYTHONPATH", directory.path().as_os_str()),
        ("PET_SHUTDOWN_CONTROL", control_path.as_os_str()),
        ("PET_SHUTDOWN_LEASE", lease_path.as_os_str()),
        ("PET_SHUTDOWN_READY", ready_path.as_os_str()),
    ])
    .unwrap();
    client.info().unwrap();
    let worker = client.clone();
    let request = thread::spawn(move || worker.resolve(&python));
    let started = Instant::now();
    while !ready_path.is_file() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "probe must establish its descendant lease; stderr: {}",
            client.stderr_output()
        );
        thread::sleep(Duration::from_millis(10));
    }
    let lease = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lease_path)
        .unwrap();
    assert!(matches!(
        lease.try_lock(),
        Err(fs::TryLockError::WouldBlock)
    ));
    let started = Instant::now();
    let status = client.shutdown(Duration::from_secs(4)).unwrap();
    assert!(
        status.success(),
        "active-probe shutdown failed: {status}; stderr: {}",
        client.stderr_output()
    );
    // Observe OS lease release within the same budget as server shutdown.
    wait_for_descendant_lease(
        || lease.try_lock(),
        Duration::from_secs(4).saturating_sub(started.elapsed()),
    )
    .expect("shutdown must release the actual descendant's lease");
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(
        request.join().unwrap().is_err(),
        "an active request must be cancelled, not reported as successful"
    );
}
