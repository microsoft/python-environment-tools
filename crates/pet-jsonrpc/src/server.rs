// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::{send_error, RequestId};
use serde_json::{self, Value};
use std::{
    collections::HashMap,
    io::{self, BufRead, BufReader, Read},
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};

type RequestHandler<C> = Arc<dyn Fn(Arc<C>, RequestId, Value)>;
type NotificationHandler<C> = Arc<dyn Fn(Arc<C>, Value)>;
type ErrorHandler = Arc<dyn Fn(Option<&RequestId>, i32, String)>;

pub struct HandlersKeyedByMethodName<C> {
    context: Arc<C>,
    requests: HashMap<&'static str, RequestHandler<C>>,
    notifications: HashMap<&'static str, NotificationHandler<C>>,
    send_error: ErrorHandler,
}

impl<C> HandlersKeyedByMethodName<C> {
    pub fn new(context: Arc<C>) -> Self {
        HandlersKeyedByMethodName {
            context,
            requests: HashMap::new(),
            notifications: HashMap::new(),
            send_error: Arc::new(send_error),
        }
    }

    #[cfg(test)]
    fn new_with_error_handler(
        context: Arc<C>,
        send_error: impl Fn(Option<&RequestId>, i32, String) + 'static,
    ) -> Self {
        HandlersKeyedByMethodName {
            context,
            requests: HashMap::new(),
            notifications: HashMap::new(),
            send_error: Arc::new(send_error),
        }
    }

    pub fn add_request_handler<F>(&mut self, method: &'static str, handler: F)
    where
        F: Fn(Arc<C>, RequestId, Value) + Send + Sync + 'static,
    {
        self.requests.insert(
            method,
            Arc::new(move |context, id, params| {
                handler(context, id, params);
            }),
        );
    }

    pub fn add_notification_handler<F>(&mut self, method: &'static str, handler: F)
    where
        F: Fn(Arc<C>, Value) + Send + Sync + 'static,
    {
        self.notifications.insert(
            method,
            Arc::new(move |context, params| {
                handler(context, params);
            }),
        );
    }

    fn handle_request(&self, message: Value) {
        let id = match message.get("id") {
            None => None,
            Some(Value::String(id)) => Some(RequestId::String(id.clone())),
            Some(Value::Number(id)) => Some(RequestId::Number(id.clone())),
            Some(Value::Null) => Some(RequestId::Null),
            Some(_) => {
                (self.send_error)(None, -32600, "Invalid JSONRPC request ID".to_string());
                return;
            }
        };
        match message["method"].as_str() {
            Some(method) => {
                if let Some(id) = id {
                    if let Some(handler) = self.requests.get(method) {
                        handler(self.context.clone(), id, message["params"].clone());
                    } else {
                        eprint!("Failed to find handler for method: {method}");
                        (self.send_error)(
                            Some(&id),
                            -1,
                            format!("Failed to find handler for request {method}"),
                        );
                    }
                } else if let Some(handler) = self.notifications.get(method) {
                    handler(self.context.clone(), message["params"].clone());
                } else {
                    eprint!("Failed to find handler for method: {method}");
                }
            }
            None => {
                eprint!("Failed to get method from message: {message}");
                (self.send_error)(
                    id.as_ref(),
                    -3,
                    format!("Failed to extract method from JSONRPC payload {message:?}"),
                );
            }
        };
    }
}

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(20);
const MAX_HEADER_BYTES: usize = 8 * 1024;
const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

/// Runs the standalone process transport until EOF or a fatal I/O error.
/// Pending output is discarded at shutdown; callers must finish subprocess cleanup
/// and exit the process rather than join workers blocked in external I/O.
pub fn start_server<C>(handlers: &HandlersKeyedByMethodName<C>) -> io::Result<()> {
    crate::initialize_output()?;
    let result = (|| {
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("pet-jsonrpc-input".to_string())
            .spawn(move || read_input(BufReader::new(io::stdin()), sender))?;
        dispatch_input(handlers, &receiver, crate::output_error)
    })();
    close_transport(result, crate::shutdown_output, crate::output_error)
}

fn close_transport(
    result: io::Result<()>,
    shutdown_output: impl FnOnce(),
    output_error: impl FnOnce() -> Option<io::Error>,
) -> io::Result<()> {
    shutdown_output();
    result.and(output_error().map_or(Ok(()), Err))
}

fn read_input(mut reader: impl BufRead, sender: mpsc::SyncSender<io::Result<Option<Vec<u8>>>>) {
    loop {
        let frame = read_payload(&mut reader);
        let terminal = !matches!(&frame, Ok(Some(_)));
        if sender.send(frame).is_err() || terminal {
            return;
        }
    }
}

fn dispatch_input<C>(
    handlers: &HandlersKeyedByMethodName<C>,
    receiver: &mpsc::Receiver<io::Result<Option<Vec<u8>>>>,
    output_error: impl Fn() -> Option<io::Error>,
) -> io::Result<()> {
    loop {
        if let Some(error) = output_error() {
            return Err(error);
        }
        match receiver.recv_timeout(INPUT_POLL_INTERVAL) {
            Ok(Ok(Some(payload))) => {
                if handle_payload(handlers, &payload).is_err() {
                    (handlers.send_error)(None, -32700, "Invalid JSONRPC JSON payload".to_string());
                }
            }
            Ok(Ok(None)) => return Ok(()),
            Ok(Err(error)) => return Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "JSONRPC input reader stopped without a terminal result",
                ));
            }
        }
    }
}

fn read_payload(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut header = String::new();
    let count = (&mut *reader)
        .take(MAX_HEADER_BYTES as u64 + 1)
        .read_line(&mut header)?;
    if count == 0 {
        return Ok(None);
    }
    if count > MAX_HEADER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "JSONRPC header exceeds limit",
        ));
    }
    if !header.ends_with('\n') {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Truncated JSONRPC header",
        ));
    }
    let length = get_content_length(&header)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if length > MAX_PAYLOAD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "JSONRPC payload exceeds limit",
        ));
    }
    let mut separator = String::new();
    (&mut *reader)
        .take((MAX_HEADER_BYTES - count) as u64 + 1)
        .read_line(&mut separator)?;
    if count + separator.len() > MAX_HEADER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "JSONRPC header exceeds limit",
        ));
    }
    if !separator.ends_with('\n') {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Truncated JSONRPC header separator",
        ));
    }
    if separator != "\r\n" && separator != "\n" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Expected a blank JSONRPC header separator",
        ));
    }
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(length)
        .map_err(io::Error::other)?;
    payload.resize(length, 0);
    reader.read_exact(&mut payload)?;
    Ok(Some(payload))
}

fn handle_payload<C>(
    handlers: &HandlersKeyedByMethodName<C>,
    payload: impl AsRef<[u8]>,
) -> Result<(), serde_json::Error> {
    let request = serde_json::from_slice(payload.as_ref())?;
    handlers.handle_request(request);
    Ok(())
}

/// Parses the content length from the given line.
fn get_content_length(line: &str) -> Result<usize, String> {
    let line = line.trim();
    if let Some(content_length) = line.find("Content-Length: ") {
        let start = content_length + "Content-Length: ".len();
        if let Ok(length) = line[start..].parse::<usize>() {
            Ok(length)
        } else {
            Err(format!(
                "Failed to parse content length from {} for {}",
                &line[start..],
                line
            ))
        }
    } else {
        Err(format!(
            "String 'Content-Length' not found in input => {line}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    #[test]
    fn output_failure_between_eof_and_close_is_not_reported_as_success() {
        let closed = std::cell::Cell::new(false);
        let result = close_transport(
            Ok(()),
            || closed.set(true),
            || {
                assert!(
                    closed.get(),
                    "output must be closed before its final error is read"
                );
                Some(io::Error::from(io::ErrorKind::BrokenPipe))
            },
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn transport_error_remains_primary_and_still_closes_output() {
        let closed = std::cell::Cell::new(false);
        let result = close_transport(
            Err(io::Error::from(io::ErrorKind::InvalidData)),
            || closed.set(true),
            || Some(io::Error::from(io::ErrorKind::BrokenPipe)),
        );
        assert!(closed.get());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert!(close_transport(Ok(()), || {}, || None).is_ok());
    }

    #[derive(Default)]
    struct TestContext {
        request: Mutex<Option<(RequestId, Value)>>,
        notification: Mutex<Option<Value>>,
        errors: Mutex<Vec<(Option<RequestId>, i32, String)>>,
    }

    fn create_handlers_with_recorded_errors(
        context: Arc<TestContext>,
    ) -> HandlersKeyedByMethodName<TestContext> {
        let error_context = context.clone();
        HandlersKeyedByMethodName::new_with_error_handler(context, move |id, code, message| {
            error_context
                .errors
                .lock()
                .unwrap()
                .push((id.cloned(), code, message));
        })
    }

    #[test]
    fn request_ids_preserve_values_for_dispatch_and_errors() {
        for value in [
            json!("request-1"),
            json!(""),
            json!("\u{03c0}-request"),
            json!(-7),
            json!(0),
            json!(u64::from(u32::MAX) + 1),
            json!(u64::MAX),
            json!(i64::MIN),
            json!(1.5),
            Value::Null,
        ] {
            let id = serde_json::from_value::<RequestId>(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&id).unwrap(), value);
            let context = Arc::new(TestContext::default());
            let mut handlers = create_handlers_with_recorded_errors(context.clone());
            handlers.add_request_handler("method", |context, id, params| {
                *context.request.lock().unwrap() = Some((id, params));
            });
            handlers.add_notification_handler("method", |context, params| {
                *context.notification.lock().unwrap() = Some(params);
            });
            handlers.handle_request(json!({"id": value, "method": "method", "params": 42}));
            assert_eq!(
                *context.request.lock().unwrap(),
                Some((id.clone(), json!(42)))
            );
            assert!(context.notification.lock().unwrap().is_none());
            handlers.handle_request(json!({"method": "method", "params": 7}));
            assert_eq!(context.notification.lock().unwrap().take(), Some(json!(7)));
            handlers.handle_request(json!({"id": value, "method": "unknown"}));
            handlers.handle_request(json!({"id": value}));
            let errors = context.errors.lock().unwrap();
            assert_eq!(errors.len(), 2);
            assert_eq!(errors[0].0, Some(id.clone()));
            assert_eq!(errors[0].1, -1);
            assert_eq!(errors[1].0, Some(id));
            assert_eq!(errors[1].1, -3);
        }
    }

    #[test]
    fn missing_id_is_a_notification_but_invalid_ids_are_rejected() {
        let context = Arc::new(TestContext::default());
        let mut handlers = create_handlers_with_recorded_errors(context.clone());
        handlers.add_request_handler("method", |context, id, params| {
            *context.request.lock().unwrap() = Some((id, params));
        });
        handlers.add_notification_handler("method", |context, params| {
            *context.notification.lock().unwrap() = Some(params);
        });
        handlers.handle_request(json!({"method": "method", "params": 7}));
        assert_eq!(context.notification.lock().unwrap().take(), Some(json!(7)));
        assert!(context.request.lock().unwrap().is_none());
        assert!(context.errors.lock().unwrap().is_empty());
        for value in [json!(true), json!(false), json!([]), json!({"id": 1})] {
            assert!(serde_json::from_value::<RequestId>(value.clone()).is_err());
            handlers.handle_request(json!({"id": value, "method": "method"}));
            assert!(context.notification.lock().unwrap().is_none());
            assert!(context.request.lock().unwrap().is_none());
            assert_eq!(
                context.errors.lock().unwrap().pop(),
                Some((None, -32600, "Invalid JSONRPC request ID".into()))
            );
        }
        assert!(context.errors.lock().unwrap().is_empty());
        handlers.handle_request(json!({"id": "after-invalid", "method": "method", "params": 42}));
        assert_eq!(
            context.request.lock().unwrap().take(),
            Some((RequestId::String("after-invalid".into()), json!(42)))
        );
        assert!(context.notification.lock().unwrap().is_none());
        assert!(context.errors.lock().unwrap().is_empty());
    }

    #[test]
    fn get_content_length_parses_valid_header() {
        assert_eq!(get_content_length("Content-Length: 42\r\n").unwrap(), 42);
    }

    #[test]
    fn get_content_length_rejects_missing_header() {
        let error = get_content_length("Content-Type: application/json\r\n").unwrap_err();

        assert!(error.contains("String 'Content-Length' not found"));
    }

    #[test]
    fn get_content_length_rejects_non_numeric_length() {
        let error = get_content_length("Content-Length: nope\r\n").unwrap_err();

        assert!(error.contains("Failed to parse content length"));
    }

    #[test]
    fn handle_request_routes_request_and_notification_messages() {
        let context = Arc::new(TestContext::default());
        let mut handlers = HandlersKeyedByMethodName::new(context.clone());
        handlers.add_request_handler("request/method", |context, id, params| {
            *context.request.lock().unwrap() = Some((id, params));
        });
        handlers.add_notification_handler("notification/method", |context, params| {
            *context.notification.lock().unwrap() = Some(params);
        });

        handlers.handle_request(json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "request/method",
            "params": { "value": 42 }
        }));
        handlers.handle_request(json!({
            "jsonrpc": "2.0",
            "method": "notification/method",
            "params": ["item"]
        }));

        assert_eq!(
            *context.request.lock().unwrap(),
            Some((7.into(), json!({ "value": 42 })))
        );
        assert_eq!(*context.notification.lock().unwrap(), Some(json!(["item"])));
    }

    #[test]
    fn handle_payload_routes_valid_jsonrpc_payload() {
        let context = Arc::new(TestContext::default());
        let mut handlers = HandlersKeyedByMethodName::new(context.clone());
        handlers.add_request_handler("request/method", |context, id, params| {
            *context.request.lock().unwrap() = Some((id, params));
        });

        handle_payload(
            &handlers,
            r#"{"jsonrpc":"2.0","id":9,"method":"request/method","params":{"ok":true}}"#,
        )
        .unwrap();

        assert_eq!(
            *context.request.lock().unwrap(),
            Some((9.into(), json!({ "ok": true })))
        );
    }

    #[test]
    fn handle_payload_rejects_malformed_json_without_dispatching() {
        let context = Arc::new(TestContext::default());
        let mut handlers = HandlersKeyedByMethodName::new(context.clone());
        handlers.add_request_handler("request/method", |context, id, params| {
            *context.request.lock().unwrap() = Some((id, params));
        });

        let error = handle_payload(
            &handlers,
            r#"{"jsonrpc":"2.0","id":9,"method":"request/method","params": "#,
        )
        .unwrap_err();

        assert!(error.is_eof());
        assert!(context.request.lock().unwrap().is_none());
    }

    #[test]
    fn handle_request_reports_unknown_methods_without_invoking_known_handlers() {
        let context = Arc::new(TestContext::default());
        let mut handlers = create_handlers_with_recorded_errors(context.clone());
        handlers.add_request_handler("known/request", |context, id, params| {
            *context.request.lock().unwrap() = Some((id, params));
        });
        handlers.add_notification_handler("known/notification", |context, params| {
            *context.notification.lock().unwrap() = Some(params);
        });

        handlers.handle_request(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "unknown/request",
            "params": null
        }));
        handlers.handle_request(json!({
            "jsonrpc": "2.0",
            "method": "unknown/notification",
            "params": null
        }));

        assert!(context.request.lock().unwrap().is_none());
        assert!(context.notification.lock().unwrap().is_none());
        assert_eq!(
            context.errors.lock().unwrap().as_slice(),
            &[(
                Some(1.into()),
                -1,
                "Failed to find handler for request unknown/request".to_string()
            )]
        );
    }

    #[test]
    fn handle_request_reports_missing_method_with_request_id() {
        let context = Arc::new(TestContext::default());
        let handlers = create_handlers_with_recorded_errors(context.clone());

        let message = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "params": { "value": 42 }
        });

        handlers.handle_request(message.clone());

        assert_eq!(
            context.errors.lock().unwrap().as_slice(),
            &[(
                Some(1.into()),
                -3,
                format!("Failed to extract method from JSONRPC payload {message:?}")
            )]
        );
    }

    #[test]
    fn handle_request_reports_missing_method_with_null_id() {
        let context = Arc::new(TestContext::default());
        let handlers = create_handlers_with_recorded_errors(context.clone());

        let message = json!({
            "jsonrpc": "2.0",
            "params": { "value": 42 }
        });

        handlers.handle_request(message.clone());

        assert_eq!(
            context.errors.lock().unwrap().as_slice(),
            &[(
                None,
                -3,
                format!("Failed to extract method from JSONRPC payload {message:?}")
            )]
        );
    }
    #[test]
    fn input_distinguishes_clean_eof_from_truncated_frames() {
        assert!(read_payload(&mut io::Cursor::new(b"")).unwrap().is_none());
        for bytes in [
            b"Content-Length: 2".as_slice(),
            b"Content-Length: 2\r\n".as_slice(),
            b"Content-Length: 2\r\n\r".as_slice(),
            b"Content-Length: 2\r\n\r\n{".as_slice(),
        ] {
            let error = read_payload(&mut io::Cursor::new(bytes)).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        }
    }

    #[test]
    fn input_preserves_fragmented_consecutive_payload_bytes() {
        let first = "{\"text\":\"\u{03c0}\"}".as_bytes();
        let second = b"{}";
        let mut bytes = format!("Content-Length: {}\r\n\r\n", first.len()).into_bytes();
        bytes.extend_from_slice(first);
        bytes.extend_from_slice(b"Content-Length: 2\n\n");
        bytes.extend_from_slice(second);
        let mut reader = BufReader::with_capacity(1, io::Cursor::new(bytes));
        assert_eq!(read_payload(&mut reader).unwrap().unwrap(), first);
        assert_eq!(read_payload(&mut reader).unwrap().unwrap(), second);
        assert!(read_payload(&mut reader).unwrap().is_none());
    }

    #[test]
    fn input_rejects_oversized_headers_and_payloads_before_body_reads() {
        let bytes = vec![b'x'; MAX_HEADER_BYTES + 1];
        assert_eq!(
            read_payload(&mut io::Cursor::new(bytes))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        let bytes = format!("Content-Length: {}\r\n\r\n", MAX_PAYLOAD_BYTES + 1);
        assert_eq!(
            read_payload(&mut io::Cursor::new(bytes))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn input_reader_reports_terminal_error_once() {
        struct FailedReader;
        impl Read for FailedReader {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected read failure",
                ))
            }
        }
        impl BufRead for FailedReader {
            fn fill_buf(&mut self) -> io::Result<&[u8]> {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected read failure",
                ))
            }
            fn consume(&mut self, _: usize) {}
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        read_input(FailedReader, sender);
        assert_eq!(
            receiver.recv().unwrap().unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        let (sender, receiver) = mpsc::sync_channel(1);
        read_input(io::Cursor::new(b""), sender);
        assert!(receiver.recv().unwrap().unwrap().is_none());
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn dispatch_recovers_after_malformed_json_and_stops_on_eof() {
        let context = Arc::new(TestContext::default());
        let mut handlers = create_handlers_with_recorded_errors(context.clone());
        handlers.add_request_handler("info", |context, id, params| {
            *context.request.lock().unwrap() = Some((id, params));
        });
        let (sender, receiver) = mpsc::sync_channel(3);
        sender.send(Ok(Some(b"{invalid".to_vec()))).unwrap();
        sender
            .send(Ok(Some(
                br#"{"jsonrpc":"2.0","id":"next","method":"info","params":{}}"#.to_vec(),
            )))
            .unwrap();
        sender.send(Ok(None)).unwrap();
        dispatch_input(&handlers, &receiver, || None).unwrap();
        assert_eq!(context.errors.lock().unwrap()[0].1, -32700);
        assert_eq!(
            context.request.lock().unwrap().as_ref().unwrap().0,
            RequestId::String("next".to_string())
        );
    }

    #[test]
    fn dispatch_surfaces_output_failure_without_waiting_for_stdin() {
        let handlers = create_handlers_with_recorded_errors(Arc::new(TestContext::default()));
        let (_sender, receiver) = mpsc::sync_channel(1);
        let error = dispatch_input(&handlers, &receiver, || {
            Some(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "injected output failure",
            ))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
}
