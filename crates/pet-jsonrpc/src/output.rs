// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use serde::Serialize;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

const MAX_QUEUED_BYTES: usize = 32 * 1024 * 1024;
const MAX_QUEUED_FRAMES: usize = 1024;
const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const HEADER_PREFIX: &[u8] = b"Content-Length: ";
const HEADER_SUFFIX: &[u8] = b"\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n";

#[derive(Clone, Debug)]
struct ErrorSnapshot {
    kind: io::ErrorKind,
    message: Arc<str>,
}

impl ErrorSnapshot {
    fn new(error: io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: Arc::from(error.to_string()),
        }
    }

    fn to_error(&self) -> io::Error {
        io::Error::new(self.kind, self.message.to_string())
    }
}

struct State {
    accepting: bool,
    queue: VecDeque<Vec<u8>>,
    queued_bytes: usize,
    error: Option<ErrorSnapshot>,
}

impl State {
    fn open() -> Self {
        Self {
            accepting: true,
            queue: VecDeque::new(),
            queued_bytes: 0,
            error: None,
        }
    }

    fn fail(&mut self, error: io::Error) {
        if !self.accepting {
            return;
        }
        self.error = Some(ErrorSnapshot::new(error));
        self.accepting = false;
        self.queue.clear();
        self.queued_bytes = 0;
    }
}

struct Shared {
    state: Mutex<State>,
    ready: Condvar,
}

#[derive(Clone, Copy)]
struct Limits {
    queued_bytes: usize,
    queued_frames: usize,
    payload_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            queued_bytes: MAX_QUEUED_BYTES,
            queued_frames: MAX_QUEUED_FRAMES,
            payload_bytes: MAX_PAYLOAD_BYTES,
        }
    }
}

#[derive(Clone)]
struct Output {
    shared: Arc<Shared>,
    serialization: Arc<Mutex<()>>,
    limits: Limits,
}

impl Output {
    fn new(writer: impl Write + Send + 'static) -> io::Result<Self> {
        Self::with_limits(writer, Limits::default())
    }

    fn with_limits(writer: impl Write + Send + 'static, limits: Limits) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::open()),
            ready: Condvar::new(),
        });
        let writer_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("pet-jsonrpc-output".to_string())
            .spawn(move || writer_loop(writer, writer_shared))?;

        Ok(Self {
            shared,
            serialization: Arc::new(Mutex::new(())),
            limits,
        })
    }

    fn send<T: Serialize>(&self, value: &T) {
        if !self.is_accepting() {
            return;
        }

        let _serialization = self
            .serialization
            .lock()
            .expect("JSONRPC output serialization lock poisoned");
        if !self.is_accepting() {
            return;
        }

        let frame = match encode_frame(value, self.limits.payload_bytes) {
            Ok(frame) => frame,
            Err(error) => {
                self.fail(error);
                return;
            }
        };

        let mut state = self
            .shared
            .state
            .lock()
            .expect("JSONRPC output state lock poisoned");
        if !state.accepting {
            return;
        }

        let Some(queued_bytes) = state.queued_bytes.checked_add(frame.capacity()) else {
            state.fail(io::Error::new(
                io::ErrorKind::WouldBlock,
                "JSONRPC output queue byte count overflowed",
            ));
            self.shared.ready.notify_all();
            return;
        };
        if state.queue.len() >= self.limits.queued_frames || queued_bytes > self.limits.queued_bytes
        {
            state.fail(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!(
                    "JSONRPC output queue is full (limit: {} frames and {} bytes)",
                    self.limits.queued_frames, self.limits.queued_bytes
                ),
            ));
            self.shared.ready.notify_all();
            return;
        }

        state.queued_bytes = queued_bytes;
        state.queue.push_back(frame);
        self.shared.ready.notify_one();
    }

    fn is_accepting(&self) -> bool {
        self.shared
            .state
            .lock()
            .expect("JSONRPC output state lock poisoned")
            .accepting
    }

    fn fail(&self, error: io::Error) {
        let mut state = self
            .shared
            .state
            .lock()
            .expect("JSONRPC output state lock poisoned");
        state.fail(error);
        self.shared.ready.notify_all();
    }

    fn error(&self) -> Option<io::Error> {
        self.shared
            .state
            .lock()
            .expect("JSONRPC output state lock poisoned")
            .error
            .as_ref()
            .map(ErrorSnapshot::to_error)
    }

    fn close(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .expect("JSONRPC output state lock poisoned");
        state.accepting = false;
        state.queue.clear();
        state.queued_bytes = 0;
        self.shared.ready.notify_all();
    }
}

fn writer_loop(mut writer: impl Write, shared: Arc<Shared>) {
    loop {
        let frame = {
            let mut state = shared
                .state
                .lock()
                .expect("JSONRPC output state lock poisoned");
            loop {
                if let Some(frame) = state.queue.pop_front() {
                    state.queued_bytes -= frame.capacity();
                    break frame;
                }
                if !state.accepting {
                    return;
                }
                state = shared
                    .ready
                    .wait(state)
                    .expect("JSONRPC output state lock poisoned while waiting");
            }
        };

        if let Err(error) = writer.write_all(&frame) {
            let mut state = shared
                .state
                .lock()
                .expect("JSONRPC output state lock poisoned");
            state.fail(io::Error::new(
                error.kind(),
                format!("failed to write JSONRPC output: {error}"),
            ));
            shared.ready.notify_all();
            return;
        }
        if let Err(error) = writer.flush() {
            let mut state = shared
                .state
                .lock()
                .expect("JSONRPC output state lock poisoned");
            state.fail(io::Error::new(
                error.kind(),
                format!("failed to flush JSONRPC output: {error}"),
            ));
            shared.ready.notify_all();
            return;
        }
    }
}

struct LimitedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl LimitedBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    fn reserve_for(&mut self, new_len: usize) -> io::Result<()> {
        if new_len <= self.bytes.capacity() {
            return Ok(());
        }

        let doubled_capacity = self.bytes.capacity().checked_mul(2).unwrap_or(self.limit);
        let new_capacity = doubled_capacity.max(new_len).min(self.limit);
        let additional = new_capacity.checked_sub(self.bytes.len()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "JSONRPC payload capacity calculation underflowed",
            )
        })?;
        self.bytes.try_reserve_exact(additional).map_err(|error| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                format!("failed to allocate JSONRPC payload: {error}"),
            )
        })?;
        if self.bytes.capacity() > self.limit {
            self.bytes = Vec::new();
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "JSONRPC payload allocation exceeded its maximum capacity",
            ));
        }
        Ok(())
    }
}

impl Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let new_len =
            self.bytes.len().checked_add(bytes.len()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "JSON payload too large")
            })?;
        if new_len > self.limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "JSONRPC payload exceeds the maximum size of {} bytes",
                    self.limit
                ),
            ));
        }
        self.reserve_for(new_len)?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encode_frame<T: Serialize>(value: &T, payload_limit: usize) -> io::Result<Vec<u8>> {
    let mut payload = LimitedBuffer::new(payload_limit);
    serde_json::to_writer(&mut payload, value).map_err(|error| {
        let kind = error.io_error_kind().unwrap_or(io::ErrorKind::InvalidData);
        io::Error::new(kind, format!("failed to serialize JSONRPC output: {error}"))
    })?;

    let length = payload.bytes.len().to_string();
    let frame_len = HEADER_PREFIX
        .len()
        .checked_add(length.len())
        .and_then(|len| len.checked_add(HEADER_SUFFIX.len()))
        .and_then(|len| len.checked_add(payload.bytes.len()))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "JSONRPC frame too large"))?;
    let mut frame = Vec::new();
    frame.try_reserve_exact(frame_len).map_err(|error| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            format!("failed to allocate JSONRPC frame: {error}"),
        )
    })?;
    frame.extend_from_slice(HEADER_PREFIX);
    frame.extend_from_slice(length.as_bytes());
    frame.extend_from_slice(HEADER_SUFFIX);
    frame.extend_from_slice(&payload.bytes);
    Ok(frame)
}

struct GlobalState {
    output: Option<Output>,
    shutdown: bool,
    initialization_error: Option<ErrorSnapshot>,
}

static GLOBAL: Mutex<GlobalState> = Mutex::new(GlobalState {
    output: None,
    shutdown: false,
    initialization_error: None,
});

/// Starts the process-wide JSONRPC writer using an owned duplicate of stdout.
///
/// The writer is intentionally process-lifetime infrastructure for PET's standalone
/// server. Shutdown never joins it: an OS write already in progress may remain
/// blocked until the process exits.
pub fn initialize_output() -> io::Result<()> {
    let mut global = GLOBAL
        .lock()
        .expect("global JSONRPC output state lock poisoned");
    initialize_output_state(&mut global, || duplicate_stdout().and_then(Output::new))
}

fn initialize_output_state(
    global: &mut GlobalState,
    create_output: impl FnOnce() -> io::Result<Output>,
) -> io::Result<()> {
    if global.shutdown {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "JSONRPC output has been shut down",
        ));
    }
    if let Some(error) = &global.initialization_error {
        return Err(error.to_error());
    }
    if global.output.is_some() {
        return Ok(());
    }

    let output = create_output();
    match output {
        Ok(output) => {
            global.output = Some(output);
            Ok(())
        }
        Err(error) => {
            let error = io::Error::new(
                error.kind(),
                format!("failed to initialize JSONRPC output: {error}"),
            );
            global.initialization_error = Some(ErrorSnapshot::new(error));
            Err(global
                .initialization_error
                .as_ref()
                .expect("initialization error was just stored")
                .to_error())
        }
    }
}

/// Returns a copy of the first fatal output error, if one has occurred.
pub fn output_error() -> Option<io::Error> {
    let global = GLOBAL
        .lock()
        .expect("global JSONRPC output state lock poisoned");
    global
        .initialization_error
        .as_ref()
        .map(ErrorSnapshot::to_error)
        .or_else(|| global.output.as_ref().and_then(Output::error))
}

/// Irreversibly closes process-wide output admission and discards queued frames.
///
/// This returns without joining the writer. A write already blocked in the OS is
/// left for process termination to clean up.
pub fn shutdown_output() {
    let output = {
        let mut global = GLOBAL
            .lock()
            .expect("global JSONRPC output state lock poisoned");
        shutdown_output_state(&mut global)
    };
    if let Some(output) = output {
        output.close();
    }
}

fn shutdown_output_state(global: &mut GlobalState) -> Option<Output> {
    global.shutdown = true;
    global.output.clone()
}

pub(crate) fn send<T: Serialize>(value: &T) {
    let output = {
        let mut global = GLOBAL
            .lock()
            .expect("global JSONRPC output state lock poisoned");
        if global.shutdown || global.initialization_error.is_some() {
            return;
        }
        if global.output.is_none() {
            match duplicate_stdout().and_then(Output::new) {
                Ok(output) => global.output = Some(output),
                Err(error) => {
                    let error = io::Error::new(
                        error.kind(),
                        format!("failed to initialize JSONRPC output: {error}"),
                    );
                    global.initialization_error = Some(ErrorSnapshot::new(error));
                    return;
                }
            }
        }
        global.output.clone()
    };

    if let Some(output) = output {
        output.send(value);
    }
}

#[cfg(unix)]
fn duplicate_stdout() -> io::Result<File> {
    use std::os::fd::AsFd;

    io::stdout().as_fd().try_clone_to_owned().map(File::from)
}

#[cfg(windows)]
fn duplicate_stdout() -> io::Result<File> {
    use std::os::windows::io::AsHandle;

    io::stdout()
        .as_handle()
        .try_clone_to_owned()
        .map(File::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serializer;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::time::{Duration, Instant};

    const TIMEOUT: Duration = Duration::from_secs(5);

    #[derive(Clone, Default)]
    struct CapturedWriter {
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for CapturedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes
                .lock()
                .expect("captured output lock poisoned")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct BlockingWriter {
        entered: Option<Sender<()>>,
        release: Arc<(Mutex<bool>, Condvar)>,
        bytes: Arc<Mutex<Vec<u8>>>,
        finished: Sender<()>,
    }

    impl Write for BlockingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if let Some(entered) = self.entered.take() {
                entered.send(()).expect("blocked writer observer dropped");
            }
            let (released, ready) = &*self.release;
            let mut released = released.lock().expect("release lock poisoned");
            while !*released {
                released = ready.wait(released).expect("release lock poisoned");
            }
            self.bytes
                .lock()
                .expect("captured output lock poisoned")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.finished
                .send(())
                .expect("writer completion observer dropped");
            Ok(())
        }
    }

    struct FailingWriter {
        write_error: Option<io::ErrorKind>,
        flush_error: Option<io::ErrorKind>,
    }

    impl Write for FailingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if let Some(kind) = self.write_error {
                Err(io::Error::new(kind, "deliberate write failure"))
            } else {
                Ok(bytes.len())
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            if let Some(kind) = self.flush_error {
                Err(io::Error::new(kind, "deliberate flush failure"))
            } else {
                Ok(())
            }
        }
    }

    struct DelayedFailingWriter {
        entered: Option<Sender<()>>,
        release: Arc<(Mutex<bool>, Condvar)>,
        completed: Option<Sender<()>>,
    }

    impl Write for DelayedFailingWriter {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            self.entered
                .take()
                .expect("writer entered more than once")
                .send(())
                .expect("writer observer dropped");
            let (released, ready) = &*self.release;
            let mut released = released.lock().expect("release lock poisoned");
            while !*released {
                released = ready.wait(released).expect("release lock poisoned");
            }
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "delayed write failure",
            ))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Drop for DelayedFailingWriter {
        fn drop(&mut self) {
            self.completed
                .take()
                .expect("writer completion already reported")
                .send(())
                .expect("writer completion observer dropped");
        }
    }

    fn wait_for_error(output: &Output) -> io::Error {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(error) = output.error() {
                return error;
            }
            assert!(Instant::now() < deadline, "timed out waiting for error");
            thread::yield_now();
        }
    }

    fn wait_for_len(bytes: &Arc<Mutex<Vec<u8>>>, expected: usize) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if bytes.lock().expect("captured output lock poisoned").len() >= expected {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for output");
            thread::yield_now();
        }
    }

    fn frames(bytes: &[u8]) -> Vec<&[u8]> {
        let mut remaining = bytes;
        let mut result = Vec::new();
        while !remaining.is_empty() {
            let separator = b"\r\n\r\n";
            let header_end = remaining
                .windows(separator.len())
                .position(|window| window == separator)
                .expect("frame header terminator missing");
            let header = std::str::from_utf8(&remaining[..header_end]).expect("invalid header");
            let length: usize = header
                .strip_prefix("Content-Length: ")
                .and_then(|header| header.lines().next())
                .expect("content length missing")
                .parse()
                .expect("invalid content length");
            let payload_start = header_end + separator.len();
            let payload_end = payload_start + length;
            result.push(&remaining[payload_start..payload_end]);
            remaining = &remaining[payload_end..];
        }
        result
    }

    struct BlockingHarness {
        output: Output,
        entered: Receiver<()>,
        release: Arc<(Mutex<bool>, Condvar)>,
        bytes: Arc<Mutex<Vec<u8>>>,
        finished: Receiver<()>,
    }

    fn blocking_output(limits: Limits) -> BlockingHarness {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let output = Output::with_limits(
            BlockingWriter {
                entered: Some(entered_tx),
                release: Arc::clone(&release),
                bytes: Arc::clone(&bytes),
                finished: finished_tx,
            },
            limits,
        )
        .expect("failed to create output");
        BlockingHarness {
            output,
            entered: entered_rx,
            release,
            bytes,
            finished: finished_rx,
        }
    }

    fn release_writer(release: &Arc<(Mutex<bool>, Condvar)>) {
        let (released, ready) = &**release;
        *released.lock().expect("release lock poisoned") = true;
        ready.notify_all();
    }

    #[test]
    fn frames_unicode_by_utf8_length_and_preserves_fifo() {
        let writer = CapturedWriter::default();
        let bytes = Arc::clone(&writer.bytes);
        let output = Output::new(writer).expect("failed to create output");

        output.send(&"snowman \u{2603}");
        output.send(&serde_json::json!({"sequence": 2}));

        let expected_first = serde_json::to_vec(&"snowman \u{2603}").expect("serialization failed");
        let expected_second =
            serde_json::to_vec(&serde_json::json!({"sequence": 2})).expect("serialization failed");
        let expected_len = expected_first.len()
            + expected_second.len()
            + 2 * (HEADER_PREFIX.len() + HEADER_SUFFIX.len())
            + expected_first.len().to_string().len()
            + expected_second.len().to_string().len();
        wait_for_len(&bytes, expected_len);
        output.close();

        let bytes = bytes.lock().expect("captured output lock poisoned");
        assert_eq!(frames(&bytes), vec![expected_first, expected_second]);
    }

    #[test]
    fn blocked_sink_does_not_hold_control_or_queue_locks() {
        let BlockingHarness {
            output,
            entered,
            release,
            finished,
            ..
        } = blocking_output(Limits {
            queued_bytes: 1024,
            queued_frames: 1,
            payload_bytes: 512,
        });
        output.send(&"first");
        entered.recv_timeout(TIMEOUT).expect("writer did not block");

        let start = Instant::now();
        output.send(&"pending");
        assert!(output.error().is_none());
        output.close();
        assert!(start.elapsed() < Duration::from_secs(1));

        release_writer(&release);
        finished
            .recv_timeout(TIMEOUT)
            .expect("writer did not finish");
    }

    #[test]
    fn saturation_is_fatal_and_clears_pending_output() {
        let BlockingHarness {
            output,
            entered,
            release,
            bytes,
            finished,
        } = blocking_output(Limits {
            queued_bytes: 1024,
            queued_frames: 1,
            payload_bytes: 512,
        });
        output.send(&"first");
        entered.recv_timeout(TIMEOUT).expect("writer did not block");
        output.send(&"pending");
        output.send(&"overflow");

        let error = output.error().expect("queue saturation was not recorded");
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(error.to_string().contains("queue is full"));

        release_writer(&release);
        finished
            .recv_timeout(TIMEOUT)
            .expect("writer did not finish");
        let bytes = bytes.lock().expect("captured output lock poisoned");
        let emitted = frames(&bytes);
        assert_eq!(emitted, vec![serde_json::to_vec(&"first").unwrap()]);
    }

    #[test]
    fn oversized_serialization_is_bounded_and_fatal() {
        let writer = CapturedWriter::default();
        let bytes = Arc::clone(&writer.bytes);
        let output = Output::with_limits(
            writer,
            Limits {
                queued_bytes: 128,
                queued_frames: 2,
                payload_bytes: 16,
            },
        )
        .expect("failed to create output");

        output.send(&"this payload is much too large");

        let error = output.error().expect("oversized payload was not recorded");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("maximum size"));
        assert!(bytes
            .lock()
            .expect("captured output lock poisoned")
            .is_empty());
    }

    #[test]
    fn limited_buffer_caps_capacity_during_multi_chunk_growth() {
        let mut buffer = LimitedBuffer::new(10);

        buffer.write_all(b"123456").expect("first write failed");
        assert_eq!(buffer.bytes.len(), 6);
        assert!(buffer.bytes.capacity() <= buffer.limit);

        buffer.write_all(b"7890").expect("second write failed");
        assert_eq!(buffer.bytes, b"1234567890");
        assert_eq!(buffer.bytes.capacity(), buffer.limit);
    }

    #[test]
    fn limited_buffer_accepts_exact_limit_and_rejects_over_limit() {
        let mut buffer = LimitedBuffer::new(8);
        buffer
            .write_all(b"12345678")
            .expect("exact-limit write failed");
        assert_eq!(buffer.bytes.len(), buffer.limit);
        assert!(buffer.bytes.capacity() <= buffer.limit);

        let capacity = buffer.bytes.capacity();
        let error = buffer
            .write_all(b"9")
            .expect_err("over-limit write succeeded");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(buffer.bytes, b"12345678");
        assert_eq!(buffer.bytes.capacity(), capacity);
    }

    #[test]
    fn frame_allocation_is_exact_and_payload_limit_is_enforced() {
        let payload_len = serde_json::to_vec("1234")
            .expect("failed to encode expected payload")
            .len();
        let frame = encode_frame(&"1234", payload_len).expect("exact-limit frame failed");
        assert_eq!(frame.capacity(), frame.len());

        let error = encode_frame(&"1234", payload_len - 1).expect_err("over-limit frame succeeded");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn queued_byte_limit_is_enforced_independently_of_frame_count() {
        let queued_frame_bytes = encode_frame(&"pending", 512)
            .expect("failed to encode test frame")
            .capacity();
        let BlockingHarness {
            output,
            entered,
            release,
            bytes,
            finished,
        } = blocking_output(Limits {
            queued_bytes: queued_frame_bytes,
            queued_frames: 4,
            payload_bytes: 512,
        });
        output.send(&"x");
        entered.recv_timeout(TIMEOUT).expect("writer did not block");
        output.send(&"pending");
        output.send(&"one byte too many");

        let error = output
            .error()
            .expect("queue byte saturation was not recorded");
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);

        release_writer(&release);
        finished
            .recv_timeout(TIMEOUT)
            .expect("writer did not finish");
        let bytes = bytes.lock().expect("captured output lock poisoned");
        assert_eq!(frames(&bytes), vec![serde_json::to_vec(&"x").unwrap()]);
    }

    #[test]
    fn write_failure_is_explicit_and_first_error_wins() {
        let output = Output::new(FailingWriter {
            write_error: Some(io::ErrorKind::BrokenPipe),
            flush_error: None,
        })
        .expect("failed to create output");

        output.send(&"message");
        let first = wait_for_error(&output);
        assert_eq!(first.kind(), io::ErrorKind::BrokenPipe);
        assert!(first.to_string().contains("deliberate write failure"));

        output.fail(io::Error::new(
            io::ErrorKind::ConnectionReset,
            "later failure",
        ));
        output.close();
        let preserved = output.error().expect("first error was lost");
        assert_eq!(preserved.kind(), io::ErrorKind::BrokenPipe);
        assert!(!preserved.to_string().contains("later failure"));
    }

    #[test]
    fn flush_failure_is_explicit() {
        let output = Output::new(FailingWriter {
            write_error: None,
            flush_error: Some(io::ErrorKind::ConnectionAborted),
        })
        .expect("failed to create output");

        output.send(&"message");
        let error = wait_for_error(&output);
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
        assert!(error.to_string().contains("failed to flush JSONRPC output"));
        assert!(error.to_string().contains("deliberate flush failure"));
    }

    #[test]
    fn in_flight_write_failure_after_close_is_discarded() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (completed_tx, completed_rx) = mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let output = Output::new(DelayedFailingWriter {
            entered: Some(entered_tx),
            release: Arc::clone(&release),
            completed: Some(completed_tx),
        })
        .expect("failed to create output");

        output.send(&"message");
        entered_rx
            .recv_timeout(TIMEOUT)
            .expect("writer did not start");
        output.close();
        release_writer(&release);
        completed_rx
            .recv_timeout(TIMEOUT)
            .expect("writer did not finish");

        assert!(output.error().is_none());
    }

    #[test]
    fn close_discards_pending_frames_without_joining_writer() {
        let BlockingHarness {
            output,
            entered,
            release,
            bytes,
            finished,
        } = blocking_output(Limits {
            queued_bytes: 1024,
            queued_frames: 1,
            payload_bytes: 512,
        });
        output.send(&"in progress");
        entered.recv_timeout(TIMEOUT).expect("writer did not block");
        output.send(&"pending");

        let start = Instant::now();
        output.close();
        assert!(start.elapsed() < Duration::from_secs(1));

        release_writer(&release);
        finished
            .recv_timeout(TIMEOUT)
            .expect("writer did not finish");
        let bytes = bytes.lock().expect("captured output lock poisoned");
        assert_eq!(
            frames(&bytes),
            vec![serde_json::to_vec(&"in progress").unwrap()]
        );
    }

    struct CountedSerialization(Arc<AtomicUsize>);

    impl Serialize for CountedSerialization {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            self.0.fetch_add(1, Ordering::SeqCst);
            serializer.serialize_str("unexpected")
        }
    }

    struct DelayedFailingSerialization {
        entered: Sender<()>,
        release: Arc<(Mutex<bool>, Condvar)>,
    }

    impl Serialize for DelayedFailingSerialization {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            self.entered
                .send(())
                .expect("serialization observer dropped");
            let (released, ready) = &*self.release;
            let mut released = released.lock().expect("release lock poisoned");
            while !*released {
                released = ready.wait(released).expect("release lock poisoned");
            }
            Err(<S::Error as serde::ser::Error>::custom(
                "delayed serialization failure",
            ))
        }
    }

    #[test]
    fn in_flight_serialization_failure_after_close_is_discarded() {
        let output = Output::new(CapturedWriter::default()).expect("failed to create output");
        let (entered_tx, entered_rx) = mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let serialization = DelayedFailingSerialization {
            entered: entered_tx,
            release: Arc::clone(&release),
        };
        let publisher = {
            let output = output.clone();
            thread::spawn(move || output.send(&serialization))
        };

        entered_rx
            .recv_timeout(TIMEOUT)
            .expect("serialization did not start");
        output.close();
        release_writer(&release);
        publisher.join().expect("publisher panicked");

        assert!(output.error().is_none());
    }

    #[test]
    fn post_close_send_does_not_serialize_or_write() {
        let writer = CapturedWriter::default();
        let bytes = Arc::clone(&writer.bytes);
        let output = Output::new(writer).expect("failed to create output");
        output.close();
        let serializations = Arc::new(AtomicUsize::new(0));

        output.send(&CountedSerialization(Arc::clone(&serializations)));

        assert_eq!(serializations.load(Ordering::SeqCst), 0);
        assert!(bytes
            .lock()
            .expect("captured output lock poisoned")
            .is_empty());
        assert!(output.error().is_none());
    }

    #[test]
    fn concurrent_messages_are_complete_and_never_interleaved() {
        let writer = CapturedWriter::default();
        let bytes = Arc::clone(&writer.bytes);
        let output = Output::new(writer).expect("failed to create output");
        let messages = 32;
        let mut publishers = Vec::new();
        for sequence in 0..messages {
            let output = output.clone();
            publishers.push(thread::spawn(move || {
                output.send(&serde_json::json!({
                    "sequence": sequence,
                    "text": "\u{2603}".repeat(32),
                }));
            }));
        }
        for publisher in publishers {
            publisher.join().expect("publisher panicked");
        }

        let expected_payload_len: usize = (0..messages)
            .map(|sequence| {
                serde_json::to_vec(&serde_json::json!({
                    "sequence": sequence,
                    "text": "\u{2603}".repeat(32),
                }))
                .unwrap()
                .len()
            })
            .sum();
        let minimum_len =
            expected_payload_len + messages * (HEADER_PREFIX.len() + HEADER_SUFFIX.len() + 1);
        wait_for_len(&bytes, minimum_len);
        output.close();

        let bytes = bytes.lock().expect("captured output lock poisoned");
        let mut sequences = frames(&bytes)
            .into_iter()
            .map(|payload| {
                serde_json::from_slice::<serde_json::Value>(payload)
                    .expect("interleaved or invalid JSON payload")["sequence"]
                    .as_u64()
                    .expect("sequence missing")
            })
            .collect::<Vec<_>>();
        sequences.sort_unstable();
        assert_eq!(sequences, (0..messages as u64).collect::<Vec<_>>());
    }

    #[test]
    fn repeated_initialization_after_shutdown_returns_broken_pipe() {
        let mut global = GlobalState {
            output: None,
            shutdown: false,
            initialization_error: None,
        };
        initialize_output_state(&mut global, || Output::new(CapturedWriter::default()))
            .expect("initial initialization failed");

        let output = shutdown_output_state(&mut global).expect("initialized output missing");
        output.close();
        let error = initialize_output_state(&mut global, || {
            panic!("shutdown output must not be reinitialized")
        })
        .expect_err("initialization after shutdown succeeded");

        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert!(global.initialization_error.is_none());
        assert!(output.error().is_none());
    }
}
