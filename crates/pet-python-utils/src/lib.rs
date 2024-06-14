// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

pub mod env;
pub mod executable;
mod headers;
pub mod pyvenv_cfg;
pub mod version;

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
