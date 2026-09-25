// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use log::{error, trace};
use pet_core::python_environment::PythonEnvironment;
use pet_python_utils::{
    executable::new_silent_command,
    process::{output, ProcessError, DEFAULT_TIMEOUT},
};
use regex::Regex;
use std::{
    path::PathBuf,
    process::{Command, Output},
    time::{Duration, SystemTime},
};

use crate::{environment::create_poetry_env, manager::PoetryManager};

lazy_static! {
    static ref SANITIZE_NAME: Regex = Regex::new("[ $`!*@\"\\\r\n\t]")
        .expect("Error generating RegEx for poetry file path hash generator");
}

pub fn list_environments(
    executable: &PathBuf,
    workspace_dirs: &Vec<PathBuf>,
    manager: &PoetryManager,
) -> Vec<PythonEnvironment> {
    let mut envs = vec![];
    for workspace_dir in workspace_dirs {
        if let Some(workspace_envs) = get_environments(executable, workspace_dir) {
            for workspace_env in workspace_envs {
                if let Some(env) =
                    create_poetry_env(&workspace_env, workspace_dir.clone(), Some(manager.clone()))
                {
                    envs.push(env);
                }
            }
        }
    }
    envs
}

fn get_environments(executable: &PathBuf, workspace_dir: &PathBuf) -> Option<Vec<PathBuf>> {
    run_poetry(
        executable,
        workspace_dir,
        &["env", "list", "--full-path"],
        output,
    )
    .map(|output| parse_environments(&output))
}

fn parse_environments(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .map(str::trim)
        .map(|line| line.trim_end_matches(" (Activated)").trim())
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[derive(Clone, Debug)]
pub struct PoetryConfig {
    pub cache_dir: Option<PathBuf>,
    pub virtualenvs_in_project: Option<bool>,
    pub virtualenvs_path: Option<PathBuf>,
}

pub fn get_config(executable: &PathBuf, workspace_dir: &PathBuf) -> PoetryConfig {
    let cache_dir = get_config_path(executable, workspace_dir, "cache-dir");
    let virtualenvs_path = get_config_path(executable, workspace_dir, "virtualenvs.path");
    let virtualenvs_in_project =
        get_config_bool(executable, workspace_dir, "virtualenvs.in-project");
    PoetryConfig {
        cache_dir,
        virtualenvs_in_project,
        virtualenvs_path,
    }
}

fn get_config_bool(executable: &PathBuf, workspace_dir: &PathBuf, setting: &str) -> Option<bool> {
    get_config_value(executable, workspace_dir, setting)
        .and_then(|output| parse_config_bool(&output))
}

fn parse_config_bool(output: &str) -> Option<bool> {
    match output.trim() {
        "true" => Some(true),
        "false" => Some(false),
        "null" => None,
        value => {
            error!(
                "Poetry produced an invalid boolean configuration value: {:?}",
                value
            );
            None
        }
    }
}

fn get_config_path(
    executable: &PathBuf,
    workspace_dir: &PathBuf,
    setting: &str,
) -> Option<PathBuf> {
    get_config_value(executable, workspace_dir, setting).map(|output| PathBuf::from(output.trim()))
}

fn get_config_value(
    executable: &PathBuf,
    workspace_dir: &PathBuf,
    setting: &str,
) -> Option<String> {
    run_poetry(executable, workspace_dir, &["config", setting], output)
}

fn run_poetry(
    executable: &PathBuf,
    workspace_dir: &PathBuf,
    args: &[&str],
    run: impl FnOnce(&mut Command, Duration) -> Result<Output, ProcessError>,
) -> Option<String> {
    let start = SystemTime::now();
    let result = run(
        new_silent_command(executable)
            .args(args)
            .current_dir(workspace_dir),
        DEFAULT_TIMEOUT,
    );
    trace!(
        "Executed Poetry ({}ms): {:?} {:?} in {:?}",
        start.elapsed().unwrap_or_default().as_millis(),
        executable,
        args,
        workspace_dir
    );
    match result {
        Ok(output) if output.status.success() => match String::from_utf8(output.stdout) {
            Ok(output) => Some(output),
            Err(error) => {
                error!(
                    "Poetry {:?} using {:?} in {:?} produced invalid UTF-8: {}",
                    args, executable, workspace_dir, error
                );
                None
            }
        },
        Ok(output) => {
            trace!(
                "Failed to execute Poetry {:?} using {:?} in {:?}: {}; {}",
                args,
                executable,
                workspace_dir,
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
            None
        }
        Err(ProcessError::Cancelled) => {
            trace!(
                "Cancelled Poetry probe during process shutdown: {:?}",
                executable
            );
            None
        }
        Err(error) => {
            error!(
                "Failed to execute Poetry {:?} using {:?} in {:?}: {}",
                args, executable, workspace_dir, error
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsStr, io};

    fn captured(success: bool, stdout: &[u8]) -> Output {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;
        #[cfg(unix)]
        let status = std::process::ExitStatus::from_raw(if success { 0 } else { 23 << 8 });
        #[cfg(windows)]
        let status = std::process::ExitStatus::from_raw(if success { 0 } else { 23 });
        Output {
            status,
            stdout: stdout.to_vec(),
            stderr: b"fixture diagnostics".to_vec(),
        }
    }

    #[test]
    fn poetry_probe_preserves_arguments_cwd_and_deadline() {
        let executable = PathBuf::from("custom-poetry");
        let workspace = tempfile::tempdir().unwrap();
        let workspace_dir = workspace.path().to_path_buf();
        for args in [
            vec!["env", "list", "--full-path"],
            vec!["config", "cache-dir"],
            vec!["config", "virtualenvs.path"],
            vec!["config", "virtualenvs.in-project"],
        ] {
            let text = run_poetry(&executable, &workspace_dir, &args, |command, timeout| {
                assert_eq!(command.get_program(), OsStr::new("custom-poetry"));
                assert_eq!(command.get_args().collect::<Vec<_>>(), args);
                assert_eq!(command.get_current_dir(), Some(workspace.path()));
                assert_eq!(timeout, Duration::from_secs(15));
                Ok(captured(true, b"fixture result\r\n"))
            })
            .unwrap();
            assert_eq!(text, "fixture result\r\n");
        }
    }

    #[test]
    fn poetry_rejects_failed_invalid_and_incomplete_probes() {
        let executable = PathBuf::from("poetry");
        let cwd = PathBuf::from("workspace");
        for args in [
            vec!["env", "list", "--full-path"],
            vec!["config", "cache-dir"],
        ] {
            for result in [
                Ok(captured(false, b"success-shaped output")),
                Ok(captured(true, b"\xffinvalid path")),
                Err(ProcessError::Spawn(io::Error::from(
                    io::ErrorKind::NotFound,
                ))),
                Err(ProcessError::Cancelled),
                Err(ProcessError::Io(io::Error::from(io::ErrorKind::BrokenPipe))),
                Err(ProcessError::Timeout(Duration::from_secs(15))),
                Err(ProcessError::OutputLimit(4 * 1024 * 1024)),
                Err(ProcessError::IncompleteOutput(captured(true, b"").status)),
            ] {
                assert!(run_poetry(&executable, &cwd, &args, |_, _| result).is_none());
            }
        }
    }

    #[test]
    fn poetry_output_parsing_retains_valid_paths_and_nullable_boolean_settings() {
        assert_eq!(
            parse_environments("  env one (Activated)  \r\n\nenv-two\n"),
            [PathBuf::from("env one"), PathBuf::from("env-two")]
        );
        assert!(parse_environments(" \r\n").is_empty());
        assert_eq!(
            parse_environments("warning prose\nnull\n"),
            [PathBuf::from("warning prose"), PathBuf::from("null")]
        );
        for text in ["", "path one\npath two", "null"] {
            assert_eq!(
                run_poetry(
                    &PathBuf::from("poetry"),
                    &PathBuf::from("workspace"),
                    &["config", "cache-dir"],
                    |_, _| Ok(captured(true, text.as_bytes()))
                )
                .as_deref(),
                Some(text)
            );
        }
        assert_eq!(parse_config_bool(" true\n"), Some(true));
        assert_eq!(parse_config_bool("false\r\n"), Some(false));
        assert_eq!(parse_config_bool("null\n"), None);
        for invalid in ["", "true-but-invalid", "false-extra", "broken"] {
            assert_eq!(parse_config_bool(invalid), None);
        }
    }

    #[test]
    fn manager_noisy_output_and_hang_use_real_bounded_capture() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().to_path_buf();
        let executable = workspace.join(if cfg!(windows) {
            "manager.cmd"
        } else {
            "manager"
        });
        let payload = r#"env-one"#;
        #[cfg(windows)]
        let script = format!(
            "@echo off\r\nfor /L %%i in (1,1,128) do @echo {} 1>&2\r\necho {payload}\r\n",
            "x".repeat(1024)
        );
        #[cfg(unix)]
        let script = format!(
            "#!/bin/sh\nprintf '%s' '{}' >&2\nprintf '%s\\n' '{payload}'\n",
            "x".repeat(128 * 1024)
        );
        std::fs::write(&executable, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(
            get_environments(&executable, &workspace),
            Some(vec![PathBuf::from("env-one")])
        );
        assert_eq!(
            get_config_value(&executable, &workspace, "cache-dir")
                .as_deref()
                .map(str::trim),
            Some("env-one")
        );
        #[cfg(windows)]
        let hang = "@echo off\r\n:loop\r\ngoto loop\r\n";
        #[cfg(unix)]
        let hang = "#!/bin/sh\nwhile :; do :; done\n";
        std::fs::write(&executable, hang).unwrap();
        let started = std::time::Instant::now();
        for args in [
            vec!["env", "list", "--full-path"],
            vec!["config", "cache-dir"],
        ] {
            assert!(run_poetry(&executable, &workspace, &args, |command, _| {
                let result = output(command, Duration::from_millis(200));
                assert!(matches!(result, Err(ProcessError::Timeout(_))));
                result
            })
            .is_none());
        }
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
