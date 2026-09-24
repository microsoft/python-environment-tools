// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Direct-child output capture. Descendant supervision is intentionally separate.

use std::{
    fmt,
    io::{self, Read},
    process::{Child, Command, ExitStatus, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const CAPTURE_LIMIT: usize = 4 * 1024 * 1024;
/// Shared execution budget after synchronous OS process creation returns.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(5);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub enum ProcessError {
    Spawn(io::Error),
    Io(io::Error),
    Timeout(Duration),
    OutputLimit(usize),
    IncompleteOutput(ExitStatus),
    Cleanup {
        primary: Box<ProcessError>,
        source: io::Error,
    },
}

impl ProcessError {
    fn with_cleanup(self, cleanup: io::Result<()>) -> Self {
        match cleanup {
            Ok(()) => self,
            Err(source) => Self::Cleanup {
                primary: Box::new(self),
                source,
            },
        }
    }
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "failed to spawn subprocess: {error}"),
            Self::Io(error) => write!(f, "subprocess I/O failed: {error}"),
            Self::Timeout(timeout) => write!(f, "subprocess timed out after {timeout:?}"),
            Self::OutputLimit(limit) => {
                write!(f, "subprocess output exceeded {limit} captured bytes")
            }
            Self::IncompleteOutput(status) => write!(
                f,
                "subprocess exited with {status} but output did not reach EOF before the deadline"
            ),
            Self::Cleanup { primary, source } => {
                write!(f, "{primary}; direct-child cleanup failed: {source}")
            }
        }
    }
}

impl std::error::Error for ProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) | Self::Io(error) => Some(error),
            Self::Cleanup { primary, .. } => Some(primary.as_ref()),
            Self::Timeout(_) | Self::OutputLimit(_) | Self::IncompleteOutput(_) => None,
        }
    }
}

impl From<io::Error> for ProcessError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Drain both pipes while the direct child runs, without reader threads.
///
/// The execution clock starts when synchronous OS spawn returns. At direct-child
/// exit we continue draining within the same deadline, including when another
/// process temporarily retains a write handle. Descendants cannot extend it.
/// Retained bytes are bounded; an extra byte detects overflow and is not retained.
pub fn output(command: &mut Command, timeout: Duration) -> Result<Output, ProcessError> {
    output_with_limit(command, timeout, CAPTURE_LIMIT)
}

fn output_with_limit(
    command: &mut Command,
    timeout: Duration,
    limit: usize,
) -> Result<Output, ProcessError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(ProcessError::Spawn)?;
    let started = Instant::now();
    let mut stdout = child
        .stdout
        .take()
        .expect("stdout was configured as a pipe");
    let mut stderr = child
        .stderr
        .take()
        .expect("stderr was configured as a pipe");
    let result = (|| {
        configure_pipe(&stdout)?;
        configure_pipe(&stderr)?;
        capture(
            &mut child,
            &mut stdout,
            &mut stderr,
            started,
            timeout,
            limit,
        )
    })();
    drop(stdout);
    drop(stderr);
    match result {
        Ok(output) => Ok(output),
        Err(primary) => Err(primary.with_cleanup(terminate_and_reap(child))),
    }
}

fn capture(
    child: &mut Child,
    stdout: &mut impl Pipe,
    stderr: &mut impl Pipe,
    started: Instant,
    timeout: Duration,
    limit: usize,
) -> Result<Output, ProcessError> {
    let mut captured_stdout = Vec::new();
    let mut captured_stderr = Vec::new();
    let mut stdout_eof = false;
    let mut stderr_eof = false;
    let mut status = None;
    let mut scratch = [0; 8192];
    loop {
        if status.is_none() {
            status = poll_child(child)?;
        }
        let mut progressed = false;
        if !stdout_eof {
            progressed |= drain(
                stdout,
                &mut captured_stdout,
                captured_stderr.len(),
                &mut stdout_eof,
                &mut scratch,
                limit,
            )?;
        }
        if !stderr_eof {
            progressed |= drain(
                stderr,
                &mut captured_stderr,
                captured_stdout.len(),
                &mut stderr_eof,
                &mut scratch,
                limit,
            )?;
        }
        if let Some(status) = status {
            if stdout_eof && stderr_eof {
                return Ok(Output {
                    status,
                    stdout: captured_stdout,
                    stderr: captured_stderr,
                });
            }
        }
        if started.elapsed() >= timeout {
            return Err(match status {
                Some(status) => ProcessError::IncompleteOutput(status),
                None => ProcessError::Timeout(timeout),
            });
        }
        if !progressed {
            thread::sleep(POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())));
        }
    }
}

fn drain(
    pipe: &mut (impl Pipe + ?Sized),
    captured: &mut Vec<u8>,
    other_len: usize,
    eof: &mut bool,
    scratch: &mut [u8],
    limit: usize,
) -> Result<bool, ProcessError> {
    let remaining = limit
        .saturating_sub(captured.len())
        .saturating_sub(other_len);
    let maximum = scratch.len().min(remaining.saturating_add(1));
    match pipe.read_available(&mut scratch[..maximum])? {
        PipeRead::Pending => Ok(false),
        PipeRead::Eof => {
            *eof = true;
            Ok(true)
        }
        PipeRead::Data(count) => {
            if count > remaining {
                return Err(ProcessError::OutputLimit(limit));
            }
            captured.extend_from_slice(&scratch[..count]);
            Ok(true)
        }
    }
}

enum PipeRead {
    Data(usize),
    Pending,
    Eof,
}

trait Pipe {
    fn read_available(&mut self, buffer: &mut [u8]) -> io::Result<PipeRead>;
}

macro_rules! impl_pipe {
    ($($pipe:ty),+) => {$(
        impl Pipe for $pipe {
            fn read_available(&mut self, buffer: &mut [u8]) -> io::Result<PipeRead> {
                read_available(self, buffer)
            }
        }
    )+};
}
impl_pipe!(std::process::ChildStdout, std::process::ChildStderr);

#[cfg(unix)]
fn configure_pipe(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: the descriptor is live, and both fcntl operations use integer flags.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn configure_pipe(_pipe: &impl std::os::windows::io::AsRawHandle) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn read_available(pipe: &mut impl Read, buffer: &mut [u8]) -> io::Result<PipeRead> {
    match pipe.read(buffer) {
        Ok(0) => Ok(PipeRead::Eof),
        Ok(count) => Ok(PipeRead::Data(count)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(PipeRead::Pending)
        }
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn read_available(
    pipe: &mut (impl Read + std::os::windows::io::AsRawHandle),
    buffer: &mut [u8],
) -> io::Result<PipeRead> {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::{ERROR_BROKEN_PIPE, ERROR_HANDLE_EOF},
        System::Pipes::PeekNamedPipe,
    };
    let mut available = 0;
    // SAFETY: this is a live pipe; only the available-byte count is requested.
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            &mut available,
            ptr::null_mut(),
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        return match error.raw_os_error().map(|code| code as u32) {
            Some(ERROR_BROKEN_PIPE | ERROR_HANDLE_EOF) => Ok(PipeRead::Eof),
            _ => Err(error),
        };
    }
    if available == 0 {
        return Ok(PipeRead::Pending);
    }
    let maximum = buffer.len().min(available as usize);
    match pipe.read(&mut buffer[..maximum]) {
        Ok(0) => Ok(PipeRead::Eof),
        Ok(count) => Ok(PipeRead::Data(count)),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(PipeRead::Pending),
        Err(error) => Err(error),
    }
}

fn poll_child(child: &mut Child) -> io::Result<Option<ExitStatus>> {
    match child.try_wait() {
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(None),
        result => result,
    }
}

fn terminate_and_reap(mut child: Child) -> io::Result<()> {
    match poll_child(&mut child) {
        Ok(Some(_)) => return Ok(()),
        Ok(None) => {}
        Err(error) => {
            if let Err(kill_error) = child.kill() {
                log::error!("Failed to kill probe child after wait failure: {kill_error}");
            }
            reap_later(child);
            return Err(error);
        }
    }
    let kill_error = child.kill().err();
    let started = Instant::now();
    loop {
        match poll_child(&mut child) {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(error) => {
                reap_later(child);
                return Err(error);
            }
        }
        if started.elapsed() >= CLEANUP_TIMEOUT {
            reap_later(child);
            return Err(kill_error.unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "direct child did not exit within cleanup allowance",
                )
            }));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn reap_later(mut child: Child) {
    let pid = child.id();
    // Only exceptional OS cleanup failures use a waiter; it owns no output pipes.
    if let Err(error) = thread::Builder::new()
        .name("pet-child-reaper".into())
        .spawn(move || {
            if let Err(error) = child.wait() {
                log::error!("Failed to reap probe child: {error}");
            }
        })
    {
        log::error!("Failed to start probe child reaper for PID {pid}; reaping cannot be guaranteed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write, path::Path, sync::Mutex};

    const FIXTURE_ENV: &str = "PET_CAPTURE_FIXTURE";
    const LARGE_OUTPUT: usize = 128 * 1024;
    static FIXTURES: Mutex<()> = Mutex::new(());

    fn fixture_command(mode: &str) -> Command {
        let mut command = crate::executable::new_silent_command(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "process::tests::subprocess_fixture",
            "--nocapture",
        ]);
        command.env(FIXTURE_ENV, mode);
        command
    }

    #[test]
    fn subprocess_fixture() {
        let Ok(mode) = std::env::var(FIXTURE_ENV) else {
            return;
        };
        match mode.as_str() {
            "quiet" => std::process::exit(0),
            "stdout" | "stderr" | "both" | "nonzero" => {
                if let Some(pid_file) = std::env::var_os("PET_CAPTURE_PID") {
                    fs::write(pid_file, std::process::id().to_string()).unwrap();
                }
                if mode != "stderr" {
                    io::stdout().write_all(&vec![0xfe; LARGE_OUTPUT]).unwrap();
                }
                if mode != "stdout" {
                    io::stderr().write_all(&vec![0xff; LARGE_OUTPUT]).unwrap();
                }
                std::process::exit(if mode == "nonzero" { 23 } else { 0 });
            }
            "hang" => {
                fs::write(
                    std::env::var_os("PET_CAPTURE_PID").unwrap(),
                    std::process::id().to_string(),
                )
                .unwrap();
                thread::sleep(Duration::from_secs(30));
            }
            "inherit" => {
                let mut command = fixture_command("descendant");
                command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
                let mut descendant = command.spawn().unwrap();
                let directory = std::env::var_os("PET_CAPTURE_DIRECTORY").unwrap();
                let ready = Path::new(&directory).join("ready");
                let started = Instant::now();
                while !ready.exists() {
                    if started.elapsed() > Duration::from_secs(10) {
                        descendant.kill().unwrap();
                        descendant.wait().unwrap();
                        panic!("descendant fixture did not start");
                    }
                    thread::sleep(POLL_INTERVAL);
                }
                std::process::exit(0);
            }
            "descendant" => {
                let directory = std::env::var_os("PET_CAPTURE_DIRECTORY").unwrap();
                let directory = Path::new(&directory);
                fs::write(directory.join("ready"), b"ready").unwrap();
                let started = Instant::now();
                while !directory.join("release").exists()
                    && started.elapsed() < Duration::from_secs(10)
                {
                    thread::sleep(POLL_INTERVAL);
                }
                fs::write(directory.join("done"), b"done").unwrap();
                std::process::exit(0);
            }
            _ => panic!("unknown fixture mode: {mode}"),
        }
        std::process::exit(0);
    }

    #[test]
    fn drains_more_than_pipe_capacity_on_either_or_both_streams() {
        let _guard = FIXTURES.lock().unwrap();
        for mode in ["stdout", "stderr", "both", "nonzero"] {
            let started = Instant::now();
            let result = output(&mut fixture_command(mode), Duration::from_secs(10)).unwrap();
            assert!(started.elapsed() < Duration::from_secs(10), "{mode}");
            assert_eq!(
                result.status.code(),
                Some(if mode == "nonzero" { 23 } else { 0 })
            );
            let expected_stdout = if mode == "stderr" { 0 } else { LARGE_OUTPUT };
            let expected_stderr = if mode == "stdout" { 0 } else { LARGE_OUTPUT };
            // libtest emits a small ASCII preamble before the fixture starts.
            let binary_start = result
                .stdout
                .iter()
                .position(|b| *b == 0xfe)
                .unwrap_or(result.stdout.len());
            assert!(result.stdout[..binary_start].is_ascii());
            assert_eq!(&result.stdout[binary_start..], vec![0xfe; expected_stdout]);
            assert_eq!(result.stderr, vec![0xff; expected_stderr]);
        }
    }

    #[test]
    fn timeout_and_capture_limit_reap_direct_child() {
        let _guard = FIXTURES.lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let pid = directory.path().join("pid");
        let mut command = fixture_command("hang");
        command.env("PET_CAPTURE_PID", &pid);
        let started = Instant::now();
        let error = output(&mut command, Duration::from_secs(2)).unwrap_err();
        assert!(matches!(error, ProcessError::Timeout(_)), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
        let pid: u32 = fs::read_to_string(pid)
            .expect("fixture must reach its hang")
            .parse()
            .unwrap();
        assert_child_reaped(pid);
        let limit_pid = directory.path().join("limit-pid");
        let mut command = fixture_command("both");
        command.env("PET_CAPTURE_PID", &limit_pid);
        assert!(matches!(
            output_with_limit(&mut command, Duration::from_secs(10), 100),
            Err(ProcessError::OutputLimit(100))
        ));
        let pid = fs::read_to_string(limit_pid).unwrap().parse().unwrap();
        assert_child_reaped(pid);
    }

    #[cfg(unix)]
    fn assert_child_reaped(pid: u32) {
        let mut status = 0;
        // SAFETY: waitpid targets only our already-reaped fixture child.
        assert_eq!(
            unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[cfg(windows)]
    fn assert_child_reaped(pid: u32) {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::{
            Foundation::{ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
            System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
        };
        // SAFETY: OpenProcess returns either a uniquely owned handle or null.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            assert_eq!(
                io::Error::last_os_error().raw_os_error(),
                Some(ERROR_INVALID_PARAMETER as i32)
            );
            return;
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        // SAFETY: the process handle stays alive through this nonblocking wait.
        assert_eq!(
            unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) },
            WAIT_OBJECT_0
        );
    }

    struct ReleaseDescendant(tempfile::TempDir);

    impl Drop for ReleaseDescendant {
        fn drop(&mut self) {
            fs::write(self.0.path().join("release"), b"release").unwrap();
            let started = Instant::now();
            while !self.0.path().join("done").exists() && self.0.path().join("ready").exists() {
                assert!(
                    started.elapsed() < Duration::from_secs(12),
                    "descendant did not acknowledge cleanup"
                );
                thread::sleep(POLL_INTERVAL);
            }
        }
    }

    #[test]
    fn inherited_pipe_returns_explicit_error_without_waiting_for_descendant() {
        let _guard = FIXTURES.lock().unwrap();
        let directory = ReleaseDescendant(tempfile::tempdir().unwrap());
        let mut command = fixture_command("inherit");
        command.env("PET_CAPTURE_DIRECTORY", directory.0.path());
        let started = Instant::now();
        let error = output(&mut command, Duration::from_secs(2)).unwrap_err();
        assert!(
            matches!(error, ProcessError::IncompleteOutput(status) if status.success()),
            "{error}"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!directory.0.path().join("done").exists());
        drop(directory);
    }

    struct BytesPipe(io::Cursor<Vec<u8>>);
    impl Pipe for BytesPipe {
        fn read_available(&mut self, buffer: &mut [u8]) -> io::Result<PipeRead> {
            match self.0.read(buffer)? {
                0 => Ok(PipeRead::Eof),
                count => Ok(PipeRead::Data(count)),
            }
        }
    }

    #[test]
    fn capture_limit_is_exact_across_both_streams() {
        for limit in [0, 1, 8192, 16384] {
            let mut captured = vec![42; limit / 2];
            let other_len = limit - captured.len();
            let mut eof = false;
            let mut scratch = [0; 8192];
            let mut pipe = BytesPipe(io::Cursor::new(vec![]));
            assert!(drain(
                &mut pipe,
                &mut captured,
                other_len,
                &mut eof,
                &mut scratch,
                limit
            )
            .unwrap());
            assert!(eof);
            let mut pipe = BytesPipe(io::Cursor::new(vec![1]));
            assert!(
                matches!(drain(&mut pipe, &mut captured, other_len, &mut eof, &mut scratch, limit), Err(ProcessError::OutputLimit(n)) if n == limit)
            );
            assert_eq!(captured.len() + other_len, limit);
        }
        let mut captured = Vec::new();
        let mut pipe = BytesPipe(io::Cursor::new(vec![7; 8192]));
        let mut eof = false;
        let mut scratch = [0; 8192];
        assert!(drain(
            &mut pipe,
            &mut captured,
            8192,
            &mut eof,
            &mut scratch,
            16384
        )
        .unwrap());
        assert_eq!(captured, vec![7; 8192]);
        assert!(drain(
            &mut pipe,
            &mut captured,
            8192,
            &mut eof,
            &mut scratch,
            16384
        )
        .unwrap());
        assert!(eof);
    }

    struct TemporarilyPendingPipe {
        pending_reads: usize,
        bytes: BytesPipe,
    }

    impl Pipe for TemporarilyPendingPipe {
        fn read_available(&mut self, buffer: &mut [u8]) -> io::Result<PipeRead> {
            if self.pending_reads > 0 {
                self.pending_reads -= 1;
                return Ok(PipeRead::Pending);
            }
            self.bytes.read_available(buffer)
        }
    }

    #[test]
    fn exited_child_retries_transient_pending_reads_within_deadline() {
        let _guard = FIXTURES.lock().unwrap();
        let mut child = fixture_command("quiet")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        assert!(child.wait().unwrap().success());
        let mut stdout = TemporarilyPendingPipe {
            pending_reads: 3,
            bytes: BytesPipe(io::Cursor::new(vec![0xff, 1, 2])),
        };
        let mut stderr = BytesPipe(io::Cursor::new(vec![]));
        let result = capture(
            &mut child,
            &mut stdout,
            &mut stderr,
            Instant::now(),
            Duration::from_secs(1),
            3,
        )
        .unwrap();
        assert_eq!(result.stdout, vec![0xff, 1, 2]);
        assert!(result.stderr.is_empty());
        assert!(result.status.success());
    }

    struct FailingPipe;
    impl Pipe for FailingPipe {
        fn read_available(&mut self, _buffer: &mut [u8]) -> io::Result<PipeRead> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "injected read failure",
            ))
        }
    }

    #[test]
    fn failures_are_explicit_and_cleanup_retains_primary_cause() {
        let error = output(
            &mut Command::new("pet-capture-nonexistent-executable-553"),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(matches!(error, ProcessError::Spawn(_)));
        assert!(error.to_string().contains("failed to spawn"));
        assert!(std::error::Error::source(&error).unwrap().is::<io::Error>());
        let error = drain(
            &mut FailingPipe,
            &mut Vec::new(),
            0,
            &mut false,
            &mut [0; 16],
            100,
        )
        .unwrap_err();
        assert!(matches!(error, ProcessError::Io(_)));
        let error = ProcessError::Timeout(Duration::from_secs(1)).with_cleanup(Err(
            io::Error::new(io::ErrorKind::PermissionDenied, "injected kill failure"),
        ));
        assert!(
            matches!(&error, ProcessError::Cleanup { primary, .. } if matches!(primary.as_ref(), ProcessError::Timeout(_)))
        );
        assert!(matches!(
            ProcessError::OutputLimit(1).with_cleanup(Ok(())),
            ProcessError::OutputLimit(1)
        ));
        assert!(error.to_string().contains("timed out"));
        assert!(error.to_string().contains("kill failure"));
        let source = std::error::Error::source(&error)
            .unwrap()
            .downcast_ref::<ProcessError>()
            .unwrap();
        assert!(matches!(source, ProcessError::Timeout(_)));
        assert!(std::error::Error::source(source).is_none());
    }
}
