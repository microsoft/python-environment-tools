// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, trace, warn};
use pet_fs::path::resolve_symlink;
use pet_python_utils::{
    executable::new_silent_command,
    process::{output, ProcessError, DEFAULT_TIMEOUT},
};
use std::{
    io,
    path::PathBuf,
    process::{Command, Output},
    time::Duration,
};

#[derive(Debug, serde::Deserialize)]
pub struct CondaInfo {
    pub executable: PathBuf,
    pub envs: Vec<PathBuf>,
    pub conda_prefix: Option<PathBuf>,
    pub conda_version: String,
    pub envs_dirs: Vec<PathBuf>,
    pub config_files: Vec<PathBuf>,
    pub rc_path: Option<PathBuf>,
    pub sys_rc_path: Option<PathBuf>,
    pub user_rc_path: Option<PathBuf>,
    pub root_prefix: Option<PathBuf>,
}

#[derive(Debug, serde::Deserialize)]
pub struct CondaInfoJson {
    pub envs: Option<Vec<PathBuf>>,
    pub conda_prefix: Option<PathBuf>,
    pub conda_version: Option<String>,
    pub envs_dirs: Option<Vec<PathBuf>>,
    /// This is an alias for envs_dirs
    pub envs_path: Option<Vec<PathBuf>>,
    pub config_files: Option<Vec<PathBuf>>,
    pub rc_path: Option<PathBuf>,
    pub user_rc_path: Option<PathBuf>,
    pub sys_rc_path: Option<PathBuf>,
    pub root_prefix: Option<PathBuf>,
}

impl CondaInfo {
    pub fn from(executable: Option<PathBuf>) -> Option<CondaInfo> {
        Self::from_with_runner(executable, output)
    }

    fn from_with_runner(
        executable: Option<PathBuf>,
        run: impl FnOnce(&mut Command, Duration) -> Result<Output, ProcessError>,
    ) -> Option<Self> {
        let using_default = executable.is_none();
        // Possible we got a symlink to the conda exe, first try to resolve that.
        let executable = if cfg!(windows) {
            executable.clone().unwrap_or("conda".into())
        } else {
            let executable = executable.unwrap_or("conda".into());
            resolve_symlink(&executable).unwrap_or(executable)
        };

        let result = run(
            new_silent_command(&executable).args(["info", "--json"]),
            DEFAULT_TIMEOUT,
        );
        trace!("Executed Conda: {:?} info --json", executable);
        match result {
            Ok(output) => {
                if output.status.success() {
                    match serde_json::from_slice::<CondaInfoJson>(&output.stdout) {
                        Ok(info) => {
                            let envs_path = info.envs_path.unwrap_or_default();
                            let mut envs_dirs = info.envs_dirs.unwrap_or_default();
                            envs_dirs.extend(envs_path);
                            let info = CondaInfo {
                                executable: executable.clone(),
                                envs: info.envs.unwrap_or_default(),
                                conda_prefix: info.conda_prefix,
                                root_prefix: info.root_prefix,
                                rc_path: info.rc_path,
                                sys_rc_path: info.sys_rc_path,
                                user_rc_path: info.user_rc_path,
                                envs_dirs,
                                conda_version: info.conda_version.unwrap_or_default(),
                                config_files: info.config_files.unwrap_or_default(),
                            };
                            Some(info)
                        }
                        Err(err) => {
                            error!(
                                "Conda Execution for {:?} produced an output {:?} that could not be parsed as JSON",
                                executable, err,
                            );
                            None
                        }
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    warn!(
                        "Failed to get conda info using  {:?} ({:?}) {}",
                        executable,
                        output.status.code().unwrap_or_default(),
                        stderr
                    );
                    None
                }
            }
            Err(err) => {
                if !is_missing_default_conda(using_default, &err) {
                    warn!(
                        "Failed to execute conda info using {:?}: {}",
                        executable, err
                    );
                }
                None
            }
        }
    }
}

fn is_missing_default_conda(using_default: bool, error: &ProcessError) -> bool {
    using_default
        && matches!(error, ProcessError::Spawn(source) if source.kind() == io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

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
    fn conda_probe_uses_shared_deadline_and_preserves_metadata() {
        let info = CondaInfo::from_with_runner(None, |command, timeout| {
            assert_eq!(command.get_program(), OsStr::new("conda"));
            assert_eq!(command.get_args().collect::<Vec<_>>(), ["info", "--json"]);
            assert_eq!(timeout, Duration::from_secs(15));
            Ok(captured(true, br#" {
                "envs":["env-one","env-two"],"conda_prefix":"base","root_prefix":"root",
                "conda_version":"25.1.0","envs_dirs":["dir"],"envs_path":["alias"],
                "config_files":["config"],"rc_path":"rc","user_rc_path":"user","sys_rc_path":"system"
            } "#))
        }).unwrap();
        assert_eq!(info.executable, PathBuf::from("conda"));
        assert_eq!(
            info.envs,
            [PathBuf::from("env-one"), PathBuf::from("env-two")]
        );
        assert_eq!(info.conda_prefix, Some("base".into()));
        assert_eq!(info.root_prefix, Some("root".into()));
        assert_eq!(info.conda_version, "25.1.0");
        assert_eq!(
            info.envs_dirs,
            [PathBuf::from("dir"), PathBuf::from("alias")]
        );
        assert_eq!(info.config_files, [PathBuf::from("config")]);
        assert_eq!(info.rc_path, Some("rc".into()));
        assert_eq!(info.user_rc_path, Some("user".into()));
        assert_eq!(info.sys_rc_path, Some("system".into()));
    }

    #[test]
    fn conda_rejects_failed_invalid_and_incomplete_probes() {
        for (success, stdout) in [
            (false, b"{}".as_slice()),
            (true, b"{".as_slice()),
            (true, b"{\"conda_version\":\"\xff\"}".as_slice()),
        ] {
            assert!(
                CondaInfo::from_with_runner(Some("custom-conda".into()), |_, _| Ok(captured(
                    success, stdout
                )))
                .is_none()
            );
        }
        for error in [
            ProcessError::Spawn(io::Error::from(io::ErrorKind::NotFound)),
            ProcessError::Io(io::Error::from(io::ErrorKind::BrokenPipe)),
            ProcessError::Timeout(Duration::from_secs(15)),
            ProcessError::OutputLimit(4 * 1024 * 1024),
            ProcessError::IncompleteOutput(captured(true, b"").status),
        ] {
            assert!(CondaInfo::from_with_runner(None, |_, _| Err(error)).is_none());
        }
    }

    #[test]
    fn only_missing_default_conda_is_quiet() {
        let missing = ProcessError::Spawn(io::Error::from(io::ErrorKind::NotFound));
        assert!(is_missing_default_conda(true, &missing));
        assert!(!is_missing_default_conda(false, &missing));
        for error in [
            ProcessError::Spawn(io::Error::from(io::ErrorKind::PermissionDenied)),
            ProcessError::Io(io::Error::from(io::ErrorKind::NotFound)),
            ProcessError::Timeout(Duration::from_secs(15)),
            ProcessError::OutputLimit(1),
        ] {
            assert!(!is_missing_default_conda(true, &error));
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
        let payload = r#"{"conda_version":"25.1.0"}"#;
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
        let info = CondaInfo::from_with_runner(Some(executable.clone()), |command, timeout| {
            assert_eq!(timeout, Duration::from_secs(15));
            output(command, Duration::from_secs(5))
        })
        .expect("noisy Conda fixture must still resolve");
        assert_eq!(info.conda_version, "25.1.0");
        #[cfg(windows)]
        let hang = "@echo off\r\n:loop\r\ngoto loop\r\n";
        #[cfg(unix)]
        let hang = "#!/bin/sh\nwhile :; do :; done\n";
        std::fs::write(&executable, hang).unwrap();
        let started = std::time::Instant::now();
        assert!(CondaInfo::from_with_runner(Some(executable), |command, _| {
            let result = output(command, Duration::from_millis(200));
            assert!(matches!(result, Err(ProcessError::Timeout(_))));
            result
        })
        .is_none());
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn configured_conda_name_logs_not_found_while_implicit_default_is_quiet() {
        thread_local! {
            static WARNINGS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        }
        struct ProbeLogger;
        impl log::Log for ProbeLogger {
            fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
                metadata.level() == log::Level::Warn
            }
            fn log(&self, record: &log::Record<'_>) {
                if record.level() == log::Level::Warn && record.target() == "pet_conda::conda_info"
                {
                    WARNINGS.with(|count| count.set(count.get() + 1));
                }
            }
            fn flush(&self) {}
        }
        log::set_logger(&ProbeLogger).expect("Conda unit tests must have one logger");
        log::set_max_level(log::LevelFilter::Warn);
        assert!(log::log_enabled!(target: "pet_conda::conda_info", log::Level::Warn));
        for (executable, warnings) in [
            (None, 0),
            (Some("conda".into()), 1),
            (Some("custom-conda".into()), 1),
        ] {
            WARNINGS.with(|count| count.set(0));
            assert!(CondaInfo::from_with_runner(executable, |_, _| {
                Err(ProcessError::Spawn(io::Error::from(
                    io::ErrorKind::NotFound,
                )))
            })
            .is_none());
            log::logger().flush();
            WARNINGS.with(|count| assert_eq!(count.get(), warnings));
        }
    }
}
