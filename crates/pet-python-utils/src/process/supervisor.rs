// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use super::ProcessError;
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Condvar, Mutex,
    },
    time::{Duration, Instant},
};

struct State {
    closed: bool,
    active: usize,
    cleanup_failure: Option<CleanupFailure>,
}

struct CleanupFailure {
    kind: io::ErrorKind,
    message: String,
}

impl CleanupFailure {
    fn from_error(error: &io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: format!("probe cleanup failed: {error}"),
        }
    }

    fn to_error(&self) -> io::Error {
        io::Error::new(self.kind, self.message.clone())
    }
}

pub(super) struct Supervisor {
    cancelled: AtomicBool,
    state: Mutex<State>,
    completed: Condvar,
}

impl Supervisor {
    pub(super) const fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            state: Mutex::new(State {
                closed: false,
                active: 0,
                cleanup_failure: None,
            }),
            completed: Condvar::new(),
        }
    }

    pub(super) fn admit(&self) -> Result<Admission<'_>, ProcessError> {
        let mut state = self
            .state
            .lock()
            .expect("probe supervisor state lock poisoned");
        if state.closed {
            return Err(ProcessError::Cancelled);
        }
        state.active += 1;
        Ok(Admission {
            supervisor: self,
            finished: false,
        })
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(super) fn is_shutting_down(&self) -> bool {
        self.is_cancelled()
    }

    pub(super) fn shutdown(&self, timeout: Duration) -> io::Result<()> {
        let started = Instant::now();
        let mut state = self
            .state
            .lock()
            .expect("probe supervisor state lock poisoned");
        state.closed = true;
        self.cancelled.store(true, Ordering::Release);

        while state.active != 0 {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(shutdown_timeout(state.active, timeout));
            }
            let (next, wait) = self
                .completed
                .wait_timeout(state, remaining)
                .expect("probe supervisor state lock poisoned while waiting");
            state = next;
            if wait.timed_out() && state.active != 0 {
                return Err(shutdown_timeout(state.active, timeout));
            }
        }
        match &state.cleanup_failure {
            Some(failure) => Err(failure.to_error()),
            None => Ok(()),
        }
    }
}

fn shutdown_timeout(active: usize, timeout: Duration) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!("{active} probe(s) did not finish cleanup within {timeout:?}"),
    )
}

pub(super) struct Admission<'a> {
    supervisor: &'a Supervisor,
    finished: bool,
}

impl Admission<'_> {
    pub(super) fn is_cancelled(&self) -> bool {
        self.supervisor.is_cancelled()
    }

    /// Deregisters after cleanup and reports whether shutdown won the
    /// completion race while this probe was still admitted.
    pub(super) fn finish(mut self, cleanup: &io::Result<()>) -> bool {
        let mut state = self
            .supervisor
            .state
            .lock()
            .expect("probe supervisor state lock poisoned");
        let cancelled = state.closed;
        if state.cleanup_failure.is_none() {
            state.cleanup_failure = cleanup.as_ref().err().map(CleanupFailure::from_error);
        }
        state.active -= 1;
        self.finished = true;
        if state.active == 0 {
            self.supervisor.completed.notify_all();
        }
        cancelled
    }
}

impl Drop for Admission<'_> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let mut state = self
            .supervisor
            .state
            .lock()
            .expect("probe supervisor state lock poisoned");
        state.active -= 1;
        if state.active == 0 {
            self.supervisor.completed.notify_all();
        }
    }
}
