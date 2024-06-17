// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod core;
pub mod server;

pub fn send_message<T: serde::Serialize>(method: &'static str, params: Option<T>) {
    core::send_message(method, params)
}

pub fn send_reply<T: serde::Serialize>(id: u32, payload: Option<T>) {
    core::send_reply(id, payload)
}

pub fn send_error(id: Option<u32>, code: i32, message: String) {
    core::send_error(id, code, message)
}
