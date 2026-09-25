// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    io,
    process::{Child, ExitStatus},
    thread,
    time::{Duration, Instant},
};

pub fn wait_for_exit(child: &mut Child, timeout: Duration) -> io::Result<ExitStatus> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("process {} did not exit within {timeout:?}", child.id()),
            ));
        }
        thread::sleep(Duration::from_millis(10).min(remaining));
    }
}

pub fn shutdown_fixture(child: &mut Child, timeout: Duration) -> io::Result<ExitStatus> {
    match wait_for_exit(child, timeout) {
        Ok(status) => Ok(status),
        Err(error) => {
            eprintln!("Normal fixture shutdown failed; forcing owned child termination: {error}");
            child.kill()?;
            wait_for_exit(child, timeout)
        }
    }
}

pub fn join_reader(handle: thread::JoinHandle<()>, timeout: Duration) -> io::Result<()> {
    let started = Instant::now();
    while !handle.is_finished() {
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "fixture reader did not finish before its deadline",
            ));
        }
        thread::sleep(Duration::from_millis(10).min(remaining));
    }
    handle
        .join()
        .map_err(|_| io::Error::other("fixture reader panicked"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;

    #[test]
    fn fixture_ignores_input_eof() {
        if std::env::var_os("PET_TEST_SHUTDOWN_CHILD").is_some() {
            thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn fixture_shutdown_waits_normally_and_forces_only_after_timeout() {
        let mut normal = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        assert!(shutdown_fixture(&mut normal, Duration::from_secs(4))
            .unwrap()
            .success());

        let mut hanging = Command::new(std::env::current_exe().unwrap())
            .arg("fixture_ignores_input_eof")
            .env("PET_TEST_SHUTDOWN_CHILD", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let status = shutdown_fixture(&mut hanging, Duration::from_millis(200)).unwrap();
        assert!(
            !status.success(),
            "the fixture must require forced termination"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(hanging.try_wait().unwrap().is_some());
    }

    #[test]
    fn blocked_reader_times_out_without_joining_and_can_still_finish() {
        let (release, wait) = mpsc::channel();
        let (finished, completion) = mpsc::channel();
        let reader = thread::spawn(move || {
            wait.recv().unwrap();
            finished.send(()).unwrap();
        });
        let started = Instant::now();
        assert_eq!(
            join_reader(reader, Duration::from_millis(20))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut,
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        release.send(()).unwrap();
        completion.recv_timeout(Duration::from_secs(1)).unwrap();
        join_reader(thread::spawn(|| {}), Duration::from_secs(1)).unwrap();
    }
}
