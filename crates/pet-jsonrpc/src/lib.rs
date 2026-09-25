// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

mod output;
pub mod server;

pub use output::{initialize_output, output_error, shutdown_output};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Number(serde_json::Number),
    Null,
}

impl From<u32> for RequestId {
    fn from(id: u32) -> Self {
        Self::Number(id.into())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Debug)]
struct AnyMethodMessage<T> {
    pub jsonrpc: &'static str,
    pub method: &'static str,
    pub params: Option<T>,
}

pub fn send_message<T: serde::Serialize>(method: &'static str, params: Option<T>) {
    let payload = AnyMethodMessage {
        jsonrpc: "2.0",
        method,
        params,
    };
    output::send(&payload);
}

pub fn send_reply<T: serde::Serialize>(id: &RequestId, payload: Option<T>) {
    #[derive(Serialize)]
    struct Reply<'a, T> {
        jsonrpc: &'static str,
        result: Option<T>,
        id: &'a RequestId,
    }

    output::send(&Reply {
        jsonrpc: "2.0",
        result: payload,
        id,
    });
}

pub fn send_error(id: Option<&RequestId>, code: i32, message: String) {
    #[derive(Serialize)]
    struct ErrorBody {
        code: i32,
        message: String,
    }

    #[derive(Serialize)]
    struct ErrorReply<'a> {
        jsonrpc: &'static str,
        error: ErrorBody,
        id: Option<&'a RequestId>,
    }

    output::send(&ErrorReply {
        jsonrpc: "2.0",
        error: ErrorBody { code, message },
        id,
    });
}
