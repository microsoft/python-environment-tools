// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::io::{self, Write};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Debug)]
struct AnyMethodMessage<T> {
    pub jsonrpc: String,
    pub method: &'static str,
    pub params: Option<T>,
}

pub fn send_message<T: serde::Serialize>(method: &'static str, params: Option<T>) {
    let payload = AnyMethodMessage {
        jsonrpc: "2.0".to_string(),
        method,
        params,
    };
    let message = serde_json::to_string(&payload).unwrap();
    print!(
        "Content-Length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{}",
        message.len(),
        message
    );
    let _ = io::stdout().flush();
}
