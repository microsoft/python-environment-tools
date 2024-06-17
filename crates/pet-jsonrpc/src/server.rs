// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use crate::core::send_error;
use serde_json::{self, Value};
use std::{
    collections::HashMap,
    io::{self, Read},
    sync::Arc,
};

type RequestHandler<C> = Arc<dyn Fn(Arc<C>, u32, Value)>;
type NotificationHandler<C> = Arc<dyn Fn(Arc<C>, Value)>;

pub struct HandlersKeyedByMethodName<C> {
    context: Arc<C>,
    requests: HashMap<&'static str, RequestHandler<C>>,
    notifications: HashMap<&'static str, NotificationHandler<C>>,
}

impl<C> HandlersKeyedByMethodName<C> {
    pub fn new(context: Arc<C>) -> Self {
        HandlersKeyedByMethodName {
            context,
            requests: HashMap::new(),
            notifications: HashMap::new(),
        }
    }

    pub fn add_request_handler<F>(&mut self, method: &'static str, handler: F)
    where
        F: Fn(Arc<C>, u32, Value) + Send + Sync + 'static,
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
        match message["method"].as_str() {
            Some(method) => {
                if let Some(id) = message["id"].as_u64() {
                    if let Some(handler) = self.requests.get(method) {
                        handler(self.context.clone(), id as u32, message["params"].clone());
                    } else {
                        eprint!("Failed to find handler for method: {}", method);
                        send_error(
                            Some(id as u32),
                            -1,
                            format!("Failed to find handler for request {}", method),
                        );
                    }
                } else {
                    // No id, so this is a notification
                    if let Some(handler) = self.notifications.get(method) {
                        handler(self.context.clone(), message["params"].clone());
                    } else {
                        eprint!("Failed to find handler for method: {}", method);
                        send_error(
                            None,
                            -2,
                            format!("Failed to find handler for notification {}", method),
                        );
                    }
                }
            }
            None => {
                eprint!("Failed to get method from message: {}", message);
                send_error(
                    None,
                    -3,
                    format!(
                        "Failed to extract method from JSONRPC payload {:?}",
                        message
                    ),
                );
            }
        };
    }
}

/// Starts the jsonrpc server that listens for requests on stdin.
/// This function will block forever.
pub fn start_server<C>(handlers: &HandlersKeyedByMethodName<C>) -> ! {
    let mut stdin = io::stdin();
    loop {
        let mut input = String::new();
        match stdin.read_line(&mut input) {
            Ok(_) => {
                let mut empty_line = String::new();
                match get_content_length(&input) {
                    Ok(content_length) => {
                        let _ = stdin.read_line(&mut empty_line);
                        let mut buffer = vec![0; content_length];

                        match stdin.read_exact(&mut buffer) {
                            Ok(_) => {
                                let request =
                                    String::from_utf8_lossy(&buffer[..content_length]).to_string();
                                match serde_json::from_str(&request) {
                                    Ok(request) => handlers.handle_request(request),
                                    Err(err) => {
                                        eprint!("Failed to parse LINE: {}, {:?}", request, err)
                                    }
                                }
                                continue;
                            }
                            Err(err) => {
                                eprint!(
                                    "Failed to read exactly {} bytes, {:?}",
                                    content_length, err
                                )
                            }
                        }
                    }
                    Err(err) => eprint!("Failed to get content length from {}, {:?}", input, err),
                };
            }
            Err(error) => println!("Error in reading a line from stdin: {error}"),
        }
    }
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
            "String 'Content-Length' not found in input => {}",
            line
        ))
    }
}
