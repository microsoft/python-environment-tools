// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use log::{error, trace, warn};
use pet_core::{arch::Architecture, env::PythonEnv, python_environment::PythonEnvironment};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, SystemTime},
};

use crate::{
    cache::create_cache,
    executable::new_silent_command,
    process::{output, ProcessError, DEFAULT_TIMEOUT},
};

const PYTHON_INFO_JSON_SEPARATOR: &str = "093385e9-59f7-4a16-a604-14bf206256fe";
const PYTHON_INFO_CMD:&str = "import json, sys; print('093385e9-59f7-4a16-a604-14bf206256fe');print(json.dumps({'version': '.'.join(str(n) for n in sys.version_info), 'sys_prefix': sys.prefix, 'executable': sys.executable, 'is64_bit': sys.maxsize > 2**32}))";

/// Maximum execution time after synchronous interpreter spawn returns.
const RESOLVE_SPAWN_TIMEOUT: Duration = DEFAULT_TIMEOUT;

#[derive(Debug, Deserialize, Clone)]
pub struct InterpreterInfo {
    pub version: String,
    pub sys_prefix: String,
    pub executable: String,
    pub is64_bit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPythonEnv {
    pub executable: PathBuf,
    pub prefix: PathBuf,
    pub version: String,
    pub is64_bit: bool,
    pub symlinks: Option<Vec<PathBuf>>,
}

impl ResolvedPythonEnv {
    pub fn to_python_env(&self) -> PythonEnv {
        let mut env = PythonEnv::new(
            self.executable.clone(),
            Some(self.prefix.clone()),
            Some(self.version.clone()),
        );
        env.symlinks.clone_from(&self.symlinks);
        env
    }
    pub fn add_to_cache(&self, environment: PythonEnvironment) {
        // Verify whether we have been given the right exe.
        let arch = Some(if self.is64_bit {
            Architecture::X64
        } else {
            Architecture::X86
        });
        let symlinks = environment.symlinks.clone().unwrap_or_default();
        if symlinks.contains(&self.executable)
            && environment.version.clone().unwrap_or_default() == self.version
            && environment.prefix.clone().unwrap_or_default() == self.prefix
            && environment.arch == arch
        {
            let cache = create_cache(self.executable.clone());
            let entry = cache.lock().expect("cache mutex poisoned");
            entry.track_symlinks(symlinks)
        } else {
            error!(
                "Invalid Python environment being cached: {:?} expected {:?}",
                environment, self
            );
        }
    }
    /// Given the executable path, resolve the python environment by spawning python.
    /// If we had previously spawned Python and we have the symlinks to this as well,
    /// & all of them are the same as when this exe was previously spawned,
    /// & mtime & ctimes of none of the exes (symlinks) have changed, then we can use the cached info.
    pub fn from(
        executable: &Path,
        // known_symlinks: &Vec<PathBuf>,
        // cache: &dyn Cache,
    ) -> Option<Self> {
        let cache = create_cache(executable.to_path_buf());
        let entry = cache.lock().expect("cache mutex poisoned");
        if let Some(env) = entry.get_for_executable(executable) {
            Some(env)
        } else if let Some(env) = get_interpreter_details(executable) {
            entry.store(env.clone());
            Some(env)
        } else {
            None
        }
    }
}

fn get_interpreter_details(executable: &Path) -> Option<ResolvedPythonEnv> {
    get_interpreter_details_with_timeout(executable, RESOLVE_SPAWN_TIMEOUT)
}

fn get_interpreter_details_with_timeout(
    executable: &Path,
    timeout: Duration,
) -> Option<ResolvedPythonEnv> {
    get_interpreter_details_with_runner(executable, timeout, output)
}

fn get_interpreter_details_with_runner(
    executable: &Path,
    timeout: Duration,
    run: impl FnOnce(&mut Command, Duration) -> Result<Output, ProcessError>,
) -> Option<ResolvedPythonEnv> {
    // Spawn the python exe and get the version, sys.prefix and sys.executable.
    let executable = executable.to_str()?;
    let start = SystemTime::now();
    trace!("Executing Python: {} -c {}", executable, PYTHON_INFO_CMD);
    let result = run(
        new_silent_command(executable).args(["-c", PYTHON_INFO_CMD]),
        timeout,
    );
    match result {
        Ok(output) => parse_interpreter_result(executable, &output, start),
        Err(ProcessError::Timeout(timeout)) => {
            warn!("Timed out after {:?} resolving Python via spawn for {:?}; terminated direct child.", timeout, executable);
            None
        }
        Err(error) => {
            error!(
                "Failed to execute Python to resolve info {:?}: {}",
                executable, error
            );
            None
        }
    }
}

fn parse_interpreter_result(
    executable: &str,
    output: &std::process::Output,
    start: SystemTime,
) -> Option<ResolvedPythonEnv> {
    if !output.status.success() {
        error!(
            "Python interpreter {:?} exited with {}: {}",
            executable,
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }
    parse_interpreter_output(executable, &output.stdout, start)
}

fn parse_interpreter_output(
    executable: &str,
    output: &[u8],
    start: SystemTime,
) -> Option<ResolvedPythonEnv> {
    trace!(
        "Executed Python {:?} in {:?} & produced an output {:?}",
        executable,
        start.elapsed(),
        String::from_utf8_lossy(output)
    );
    let separator = PYTHON_INFO_JSON_SEPARATOR.as_bytes();
    if let Some(position) = output
        .windows(separator.len())
        .position(|bytes| bytes == separator)
    {
        let output = &output[position + separator.len()..];
        if let Ok(info) = serde_json::from_slice::<InterpreterInfo>(output) {
            let mut symlinks = vec![
                PathBuf::from(executable),
                PathBuf::from(info.executable.clone()),
            ];
            symlinks.sort();
            symlinks.dedup();
            Some(ResolvedPythonEnv {
                executable: PathBuf::from(info.executable.clone()),
                prefix: PathBuf::from(info.sys_prefix),
                version: info.version.trim().to_string(),
                is64_bit: info.is64_bit,
                symlinks: Some(symlinks),
            })
        } else {
            error!(
                "Python Execution for {:?} produced an output {:?} that could not be parsed as JSON",
                executable, String::from_utf8_lossy(output),
            );
            None
        }
    } else {
        error!(
            "Python Execution for {:?} produced an output {:?} without a separator",
            executable,
            String::from_utf8_lossy(output),
        );
        None
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Instant;

    // https://github.com/microsoft/python-environment-tools/issues/525:
    // A launcher printing GBK-encoded "文件不存在" must not panic discovery.
    #[test]
    fn get_interpreter_details_handles_non_utf8_stdout() {
        let result = get_interpreter_details_with_runner(
            Path::new("/bin/sh"),
            Duration::from_secs(5),
            |command, timeout| {
                assert_eq!(command.get_program(), "/bin/sh");
                assert!(command.get_args().eq(["-c", PYTHON_INFO_CMD]));
                let result = output(
                    new_silent_command("/bin/sh").args([
                        "-c",
                        r"printf '\316\304\274\376\262\273\264\346\324\332: -c\r\n'",
                    ]),
                    timeout,
                )
                .expect("non-UTF-8 interpreter fixture runner must complete");
                assert!(result.status.success());
                assert_eq!(
                    result.stdout,
                    b"\xce\xc4\xbc\xfe\xb2\xbb\xb4\xe6\xd4\xda: -c\r\n"
                );
                Ok(result)
            },
        );
        assert!(result.is_none());
    }

    #[test]
    fn noisy_interpreter_output_resolves_only_on_success() {
        let payload = format!(
            "{}\n{}",
            PYTHON_INFO_JSON_SEPARATOR,
            r#"{"version":"3.13.1","sys_prefix":"prefix","executable":"python","is64_bit":true}"#
        );
        for exit_code in [0, 23] {
            let script = format!(
                r#"i=0
while [ "$i" -lt 128 ]; do
    printf '%1024s' '' >&2
    i=$((i + 1))
done
printf '\377\376%s\n' '{payload}'
exit {exit_code}
"#
            );
            let started = Instant::now();
            let result = get_interpreter_details_with_runner(
                Path::new("/bin/sh"),
                Duration::from_secs(5),
                |command, timeout| {
                    assert_eq!(command.get_program(), "/bin/sh");
                    assert!(command.get_args().eq(["-c", PYTHON_INFO_CMD]));
                    let result =
                        output(new_silent_command("/bin/sh").args(["-c", &script]), timeout)
                            .expect("noisy interpreter fixture runner must complete");
                    assert_eq!(result.status.code(), Some(exit_code));
                    assert_eq!(result.stderr.len(), 128 * 1024);
                    assert!(result.stdout.starts_with(&[0xff, 0xfe]));
                    assert_eq!(&result.stdout[2..], format!("{payload}\n").as_bytes());
                    Ok(result)
                },
            );
            assert!(started.elapsed() < Duration::from_secs(5));
            if exit_code == 0 {
                assert_eq!(
                    result
                        .expect("successful noisy interpreter fixture must resolve")
                        .version,
                    "3.13.1"
                );
            } else {
                assert!(
                    result.is_none(),
                    "valid JSON from a failed interpreter must not be cached"
                );
            }
        }
    }

    /// Regression test for #463: a spawn that never exits must not block resolve.
    #[test]
    fn get_interpreter_details_times_out_on_hanging_executable() {
        let start = Instant::now();
        let result = get_interpreter_details_with_runner(
            Path::new("/bin/sh"),
            Duration::from_millis(200),
            |command, timeout| {
                assert_eq!(command.get_program(), "/bin/sh");
                assert!(command.get_args().eq(["-c", PYTHON_INFO_CMD]));
                let result = output(
                    new_silent_command("/bin/sh").args(["-c", "exec sleep 60"]),
                    timeout,
                );
                assert!(
                    matches!(&result, Err(ProcessError::Timeout(_))),
                    "{result:?}"
                );
                result
            },
        );
        let elapsed = start.elapsed();
        assert!(result.is_none(), "hanging spawn must return None");
        assert!(
            elapsed < Duration::from_secs(3),
            "spawn must return within the execution and cleanup budgets (took {:?})",
            elapsed
        );
    }
}

#[cfg(test)]
mod parser_tests {
    use super::*;

    #[test]
    fn preserves_non_utf8_preamble_and_unicode_json() {
        let mut bytes = vec![0xff, 0xfe, b'\n'];
        bytes.extend_from_slice(PYTHON_INFO_JSON_SEPARATOR.as_bytes());
        bytes.extend_from_slice(br#"{"version":" 3.13.1 ","sys_prefix":"C:\\env\\\u65e5","executable":"python","is64_bit":true}"#);
        let info = parse_interpreter_output("python", &bytes, SystemTime::now()).unwrap();
        assert_eq!(info.version, "3.13.1");
        assert_eq!(info.executable, PathBuf::from("python"));
        assert_eq!(info.prefix, PathBuf::from("C:\\env\\\u{65e5}"));
        assert_eq!(info.symlinks, Some(vec![PathBuf::from("python")]));
        assert!(info.is64_bit);
    }

    #[test]
    fn rejects_valid_interpreter_json_from_failed_process() {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;
        let stdout = format!(
            "{}{}",
            PYTHON_INFO_JSON_SEPARATOR,
            r#"{"version":"3.13.1","sys_prefix":"prefix","executable":"python","is64_bit":true}"#
        )
        .into_bytes();
        for code in [0, 23] {
            #[cfg(unix)]
            let status = std::process::ExitStatus::from_raw(code << 8);
            #[cfg(windows)]
            let status = std::process::ExitStatus::from_raw(code);
            let output = std::process::Output {
                status,
                stdout: stdout.clone(),
                stderr: b"fixture stderr".to_vec(),
            };
            assert_eq!(
                parse_interpreter_result("python", &output, SystemTime::now()).is_some(),
                code == 0
            );
        }
    }

    #[test]
    fn rejects_missing_separator_and_malformed_json() {
        for bytes in [
            b"\xffinvalid".as_slice(),
            PYTHON_INFO_JSON_SEPARATOR.as_bytes(),
        ] {
            assert!(parse_interpreter_output("python", bytes, SystemTime::now()).is_none());
        }
    }
}
