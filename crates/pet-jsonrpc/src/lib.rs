// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

mod core;

pub fn send_message<T: serde::Serialize>(method: &'static str, params: Option<T>) {
    core::send_message(method, params)
}
