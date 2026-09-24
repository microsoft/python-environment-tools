// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    io,
    os::unix::process::CommandExt,
    process::{Child, Command, ExitStatus},
    time::Instant,
};

pub(in crate::process) struct ProcessTree {
    group: Option<libc::pid_t>,
}

impl ProcessTree {
    pub(in crate::process) fn prepare(command: &mut Command) -> io::Result<Self> {
        command.process_group(0);
        Ok(Self { group: None })
    }

    pub(in crate::process) fn attach(&mut self, child: &Child) -> io::Result<()> {
        self.group = Some(child.id() as libc::pid_t);
        Ok(())
    }

    pub(in crate::process) fn poll(&mut self, child: &mut Child) -> io::Result<Option<ExitStatus>> {
        if self.group.is_some() {
            let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
            // WNOWAIT reserves the leader PID until we signal its group. Reaping
            // first could allow PID reuse and target an unrelated process group.
            // SAFETY: child is our unreaped child; info is writable storage.
            if unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.id(),
                    info.as_mut_ptr(),
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            } == -1
            {
                let error = io::Error::last_os_error();
                return if error.kind() == io::ErrorKind::Interrupted {
                    Ok(None)
                } else {
                    Err(error)
                };
            }
            // SAFETY: waitid succeeded; zero initialization covers the no-event case.
            if unsafe { info.assume_init().si_pid() } == 0 {
                return Ok(None);
            }
            self.terminate()?;
        }
        super::super::poll_child(child)
    }

    pub(in crate::process) fn terminate(&mut self) -> io::Result<()> {
        // Disarm even on failure: the caller must still reap the child and must
        // never signal this numeric group again after releasing its leader PID.
        let Some(group) = self.group.take() else {
            return Ok(());
        };
        // SAFETY: this is the dedicated group of our still-unreaped child.
        if unsafe { libc::kill(-group, libc::SIGKILL) } == -1 {
            let error = io::Error::last_os_error();
            #[cfg(target_os = "macos")]
            if error.raw_os_error() == Some(libc::EPERM) {
                // Darwin excludes zombies from group signalling and returns
                // EPERM when none remain signalable. Do not hide a live member
                // whose credentials actually prevent us from terminating it.
                match super::macos::zombie_only_group(group) {
                    Ok(()) => return Ok(()),
                    Err(query_error) => {
                        log::warn!("Cannot verify exited probe group: {query_error}")
                    }
                }
            }
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        Ok(())
    }

    pub(in crate::process) fn finish(&self, _started: Instant) -> io::Result<()> {
        // Grandchildren are reaped by their parent or the OS reaper, not by PET.
        Ok(())
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        if let Err(error) = self.terminate() {
            log::error!("Failed to terminate probe process group: {error}");
        }
    }
}
