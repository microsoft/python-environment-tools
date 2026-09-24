// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use super::super::{poll_child, CLEANUP_TIMEOUT, POLL_INTERVAL};
use std::{
    io,
    mem::{size_of_val, zeroed},
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Child, Command, ExitStatus},
    ptr, thread,
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::ERROR_NO_MORE_ITEMS,
    System::{
        Diagnostics::ProcessSnapshotting::{
            PssCaptureSnapshot, PssFreeSnapshot, PssWalkMarkerCreate, PssWalkMarkerFree,
            PssWalkSnapshot, HPSS, HPSSWALK, PSS_CAPTURE_THREADS, PSS_THREAD_ENTRY,
            PSS_WALK_THREADS,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
            TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        Threading::{
            GetCurrentProcess, GetProcessIdOfThread, OpenThread, ResumeThread, CREATE_NO_WINDOW,
            CREATE_SUSPENDED, THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
        },
    },
};

pub(in crate::process) struct ProcessTree {
    job: OwnedHandle,
    terminated: bool,
}

impl ProcessTree {
    pub(in crate::process) fn prepare(command: &mut Command) -> io::Result<Self> {
        // SAFETY: null arguments create an unnamed, non-inheritable job.
        let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful creation transfers unique ownership to this guard.
        let job = unsafe { OwnedHandle::from_raw_handle(job) };
        // SAFETY: the structure is POD and zero is a valid initial value.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the live handle and structure match the information class.
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
        Ok(Self {
            job,
            terminated: false,
        })
    }

    pub(in crate::process) fn attach(&mut self, child: &Child) -> io::Result<()> {
        self.assign(child)?;
        resume_child(child)
    }

    pub(in crate::process) fn assign(&self, child: &Child) -> io::Result<()> {
        // The primary thread cannot execute or spawn descendants before assignment.
        // SAFETY: both handles remain owned and live throughout this call.
        if unsafe { AssignProcessToJobObject(self.job.as_raw_handle(), child.as_raw_handle()) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(in crate::process) fn poll(&mut self, child: &mut Child) -> io::Result<Option<ExitStatus>> {
        let status = poll_child(child)?;
        if status.is_some() {
            self.terminate()?;
        }
        Ok(status)
    }

    pub(in crate::process) fn terminate(&mut self) -> io::Result<()> {
        if !self.terminated {
            // SAFETY: the unnamed job is owned exclusively by this probe.
            if unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            self.terminated = true;
        }
        Ok(())
    }

    pub(in crate::process) fn finish(&self, started: Instant) -> io::Result<()> {
        loop {
            // SAFETY: this POD output structure is initialized by the query.
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
            // SAFETY: the buffer and size match the requested information class.
            if unsafe {
                QueryInformationJobObject(
                    self.job.as_raw_handle(),
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut _,
                    size_of_val(&info) as u32,
                    ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if info.ActiveProcesses == 0 {
                return Ok(());
            }
            if started.elapsed() >= CLEANUP_TIMEOUT {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "probe job did not empty within cleanup allowance",
                ));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }
}

struct ThreadSnapshot {
    snapshot: HPSS,
    marker: HPSSWALK,
}

impl ThreadSnapshot {
    fn capture(child: &Child) -> io::Result<Self> {
        let mut snapshot = ptr::null_mut();
        // SAFETY: the child handle is live; only its thread metadata is captured,
        // not memory, contexts, handles, or a VA clone. PSS is Windows 8.1+.
        check_status(unsafe {
            PssCaptureSnapshot(child.as_raw_handle(), PSS_CAPTURE_THREADS, 0, &mut snapshot)
        })?;
        let mut captured = Self {
            snapshot,
            marker: ptr::null_mut(),
        };
        // SAFETY: null selects the default allocator; marker is writable.
        check_status(unsafe { PssWalkMarkerCreate(ptr::null(), &mut captured.marker) })?;
        Ok(captured)
    }

    fn sole_thread(&self, process_id: u32) -> io::Result<u32> {
        let mut id = None;
        loop {
            // SAFETY: PSS_THREAD_ENTRY is POD and will be populated by the walk.
            let mut entry: PSS_THREAD_ENTRY = unsafe { zeroed() };
            // SAFETY: snapshot, marker, buffer and size all match PSS_WALK_THREADS.
            let status = unsafe {
                PssWalkSnapshot(
                    self.snapshot,
                    PSS_WALK_THREADS,
                    self.marker,
                    &mut entry as *mut _ as *mut _,
                    size_of_val(&entry) as u32,
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                return id.ok_or_else(|| io::Error::other("suspended probe thread not found"));
            }
            check_status(status)?;
            if entry.ProcessId != process_id {
                return Err(io::Error::other("snapshot thread has unexpected owner"));
            }
            if id.replace(entry.ThreadId).is_some() {
                return Err(io::Error::other("suspended probe has multiple threads"));
            }
        }
    }
}

impl Drop for ThreadSnapshot {
    fn drop(&mut self) {
        if !self.marker.is_null() {
            // SAFETY: the walk marker is exclusively owned and freed once.
            if let Err(error) = check_status(unsafe { PssWalkMarkerFree(self.marker) }) {
                log::error!("Failed to free probe thread walk marker: {error}");
            }
        }
        // PssCaptureSnapshot created this descriptor in our process, even though
        // it describes the child; freeing it requires the current-process handle.
        // SAFETY: the snapshot is exclusively owned and freed once.
        if let Err(error) =
            check_status(unsafe { PssFreeSnapshot(GetCurrentProcess(), self.snapshot) })
        {
            log::error!("Failed to free probe thread snapshot: {error}");
        }
    }
}

fn check_status(status: u32) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status as i32))
    }
}

fn resume_child(child: &Child) -> io::Result<()> {
    // Stable std does not expose PROCESS_INFORMATION.hThread. A per-process PSS
    // snapshot avoids enumerating unrelated processes/threads across the system.
    let id = ThreadSnapshot::capture(child)?.sole_thread(child.id())?;
    // SAFETY: only an identifier belonging to our child was selected.
    let handle = unsafe {
        OpenThread(
            THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
            0,
            id,
        )
    };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenThread returned a uniquely owned handle.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    // Recheck using the handle, so even external termination/ID reuse fails closed.
    // SAFETY: the owned thread handle remains live through both calls.
    let owner = unsafe { GetProcessIdOfThread(handle.as_raw_handle()) };
    if owner == 0 {
        return Err(io::Error::last_os_error());
    }
    if owner != child.id() {
        return Err(io::Error::other("probe thread ownership changed"));
    }
    // SAFETY: the sole thread belongs to the child already assigned to our job.
    match unsafe { ResumeThread(handle.as_raw_handle()) } {
        1 => Ok(()),
        u32::MAX => Err(io::Error::last_os_error()),
        _ => Err(io::Error::other("unexpected probe thread suspend count")),
    }
}
