// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Architecture {
    X64,
    X86,
}

impl Ord for Architecture {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        format!("{self:?}").cmp(&format!("{other:?}"))
    }
}
impl PartialOrd for Architecture {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for Architecture {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            if *self == Architecture::X64 {
                "x64"
            } else {
                "x86"
            }
        )
        .unwrap_or_default();
        Ok(())
    }
}
