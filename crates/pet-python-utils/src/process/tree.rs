// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

#[cfg(any(target_os = "macos", test))]
mod macos;
#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(super) use unix::ProcessTree;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::ProcessTree;
