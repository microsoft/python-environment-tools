// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::io;

type ProcessBirth = (u64, u64);

fn confirmed_zombie_group(
    leader: i32,
    mut members: impl FnMut() -> io::Result<Vec<i32>>,
    mut zombie_birth: impl FnMut(i32) -> io::Result<Option<ProcessBirth>>,
) -> io::Result<()> {
    let mut before = members().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("probe group {leader} initial membership query: {error}"),
        )
    })?;
    if !before.contains(&leader) {
        return Err(io::Error::other(format!(
            "probe group {leader} snapshot omits its unreaped leader"
        )));
    }
    let mut identities = Vec::with_capacity(before.len());
    for &pid in &before {
        let birth = zombie_birth(pid)
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("probe group {leader}, PID {pid} initial status query: {error}"),
                )
            })?
            .ok_or_else(|| {
                io::Error::other(format!(
                    "probe group {leader}, PID {pid} is live or changed ownership"
                ))
            })?;
        identities.push((pid, birth));
    }
    // A live member could fork and exit between listing and status inspection.
    // Once every recorded identity is dead, confirm both membership and birth
    // timestamps: equal numeric PIDs alone do not exclude descendant PID reuse.
    let mut after = members().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("probe group {leader} final membership query: {error}"),
        )
    })?;
    before.sort_unstable();
    after.sort_unstable();
    if before != after {
        return Err(io::Error::other(format!(
            "probe group {leader} membership changed during inspection"
        )));
    }
    for (pid, birth) in identities {
        let current = zombie_birth(pid).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("probe group {leader}, PID {pid} status reinspection: {error}"),
            )
        })?;
        if current != Some(birth) {
            return Err(io::Error::other(format!(
                "probe group {leader}, PID {pid} identity changed during inspection"
            )));
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn zombie_only_group(group: libc::pid_t) -> io::Result<()> {
    confirmed_zombie_group(
        group,
        || group_members(group),
        |pid| {
            let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
            let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
            // A nonzero argument enables zombie lookup for PROC_PIDTBSDINFO.
            // SAFETY: the output buffer is aligned and sized for this flavor.
            let read = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDTBSDINFO,
                    1,
                    info.as_mut_ptr().cast(),
                    size,
                )
            };
            if read == 0 {
                return Err(io::Error::last_os_error());
            }
            if read != size {
                return Err(io::Error::other("incomplete probe member status"));
            }
            // SAFETY: proc_pidinfo populated the complete structure.
            let info = unsafe { info.assume_init() };
            Ok((info.pbi_pid == pid as u32
                && info.pbi_pgid == group as u32
                && info.pbi_status == libc::SZOMB)
                .then_some((info.pbi_start_tvsec, info.pbi_start_tvusec)))
        },
    )
}

#[cfg(target_os = "macos")]
fn group_members(group: libc::pid_t) -> io::Result<Vec<i32>> {
    // proc_info.h defines PROC_PGRP_ONLY; libc exposes the API but not this flag.
    const PROC_PGRP_ONLY: u32 = 2;
    let mut pids = [0_i32; 1024];
    let size = std::mem::size_of_val(&pids);
    // libproc returns zero for both failure and an empty list. Clear thread-local
    // errno so an empty successful snapshot cannot report a stale OS error.
    // SAFETY: __error returns this thread's writable errno slot.
    unsafe { *libc::__error() = 0 };
    // SAFETY: the group-filtered query receives a writable, bounded PID buffer.
    let read = unsafe {
        libc::proc_listpids(
            PROC_PGRP_ONLY,
            group as u32,
            pids.as_mut_ptr().cast(),
            size as i32,
        )
    };
    // The leader is still unreaped, so an empty list cannot confirm this group.
    if read <= 0 {
        let error = io::Error::last_os_error();
        return Err(if error.raw_os_error() == Some(0) {
            io::Error::other(format!("empty probe group {group} snapshot"))
        } else {
            error
        });
    }
    if read as usize >= size || !(read as usize).is_multiple_of(std::mem::size_of::<i32>()) {
        return Err(io::Error::other("incomplete probe group member list"));
    }
    Ok(pids[..read as usize / std::mem::size_of::<i32>()].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn birth(pid: i32) -> io::Result<Option<ProcessBirth>> {
        Ok(Some((10, pid as u64)))
    }

    #[test]
    fn requires_dead_members_and_unchanged_complete_membership() {
        let mut snapshots = [vec![11, 12], vec![12, 11]].into_iter();
        confirmed_zombie_group(11, || Ok(snapshots.next().unwrap()), birth).unwrap();
        assert!(confirmed_zombie_group(
            11,
            || Ok(vec![11, 12]),
            |pid| {
                if pid == 11 {
                    birth(pid)
                } else {
                    Ok(None)
                }
            }
        )
        .unwrap_err()
        .to_string()
        .contains("live or changed ownership"));
        for after in [vec![11, 12, 13], vec![11], vec![]] {
            let mut snapshots = [vec![11, 12], after].into_iter();
            assert!(
                confirmed_zombie_group(11, || Ok(snapshots.next().unwrap()), birth)
                    .unwrap_err()
                    .to_string()
                    .contains("membership changed")
            );
        }
        for before in [vec![], vec![12]] {
            assert!(confirmed_zombie_group(
                11,
                || Ok(before.clone()),
                |_| panic!("missing leader must fail closed")
            )
            .unwrap_err()
            .to_string()
            .contains("unreaped leader"));
        }
    }

    #[test]
    fn unchanged_pids_cannot_hide_replaced_or_live_processes() {
        for changed_birth in [None, Some((11, 12))] {
            let mut calls = 0;
            assert!(confirmed_zombie_group(
                11,
                || Ok(vec![11, 12]),
                |pid| {
                    calls += 1;
                    if calls == 4 {
                        Ok(changed_birth)
                    } else {
                        birth(pid)
                    }
                }
            )
            .unwrap_err()
            .to_string()
            .contains("identity changed"));
        }
    }

    #[test]
    fn inspection_errors_cannot_hide_group_signal_failure() {
        let failure = || io::Error::new(io::ErrorKind::PermissionDenied, "injected query failure");
        let initial = confirmed_zombie_group(11, || Err(failure()), birth).unwrap_err();
        assert_eq!(initial.kind(), io::ErrorKind::PermissionDenied);
        assert!(initial.to_string().contains("group 11 initial membership"));
        let status = confirmed_zombie_group(11, || Ok(vec![11]), |_| Err(failure())).unwrap_err();
        assert!(status
            .to_string()
            .contains("group 11, PID 11 initial status"));
        let mut calls = 0;
        let final_list = confirmed_zombie_group(
            11,
            || {
                calls += 1;
                if calls == 1 {
                    Ok(vec![11])
                } else {
                    Err(failure())
                }
            },
            birth,
        )
        .unwrap_err();
        assert!(final_list.to_string().contains("group 11 final membership"));
        let mut calls = 0;
        let reinspection = confirmed_zombie_group(
            11,
            || Ok(vec![11]),
            |pid| {
                calls += 1;
                if calls == 1 {
                    birth(pid)
                } else {
                    Err(failure())
                }
            },
        )
        .unwrap_err();
        assert!(reinspection
            .to_string()
            .contains("group 11, PID 11 status reinspection"));
        for error in [initial, status, final_list, reinspection] {
            assert!(error.to_string().contains("injected query failure"));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_query_recognizes_an_unreaped_zombie_leader() {
        use std::{
            os::unix::process::CommandExt,
            process::{Child, Command},
            time::{Duration, Instant},
        };
        struct ChildGuard(Option<Child>);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let Some(child) = self.0.take() else {
                    return;
                };
                if let Err(error) = super::super::super::terminate_and_reap(child, Instant::now()) {
                    eprintln!("native fixture cleanup failed: {error}");
                }
            }
        }
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 0"]).process_group(0);
        let child = ChildGuard(Some(command.spawn().unwrap()));
        let pid = child.0.as_ref().unwrap().id();
        let started = Instant::now();
        loop {
            let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
            // SAFETY: our owned child stays unreaped and info is writable.
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid,
                    info.as_mut_ptr(),
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            assert_eq!(result, 0, "{}", io::Error::last_os_error());
            // SAFETY: waitid succeeded; the no-event case remains zero-initialized.
            if unsafe { info.assume_init().si_pid() } != 0 {
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
        zombie_only_group(pid as i32)
            .expect("Darwin must expose the owned zombie with nonzero proc_pidinfo arg");
    }
}
