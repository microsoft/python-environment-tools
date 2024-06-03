// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

pub mod conda_rc;
pub mod environment_locations;
pub mod environments;
pub mod manager;
pub mod package;
pub mod utils;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
