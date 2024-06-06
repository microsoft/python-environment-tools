// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use lazy_static::lazy_static;
use regex::Regex;
use std::{fs, path::Path};

lazy_static! {
    static ref VERSION: Regex = Regex::new(r#"#define\s+PY_VERSION\s+"((\d+\.?)*.*)\""#)
        .expect("error parsing Version regex for partchlevel.h");
}

#[derive(Debug)]
pub struct Headers {
    pub version: String,
}

impl Headers {
    pub fn get_version(path: &Path) -> Option<String> {
        get_version(path)
    }
}

// Get the python version from the `<sys prefix>/include/patchlevel.h` file
// On windows the path is `<sys prefix>/Headers/patchlevel.h`
// The lines we are looking for are:
// /* Version as a string */
// #define PY_VERSION              "3.10.2"
// /*--end constants--*/
pub fn get_version(path: &Path) -> Option<String> {
    let mut path = path.to_path_buf();
    let bin = if cfg!(windows) { "Scripts" } else { "bin" };
    if path.ends_with(bin) {
        path.pop();
    }
    let headers_path = path.join(if cfg!(windows) { "Headers" } else { "include" });
    let patchlevel_h = headers_path.join("patchlevel.h");
    let mut contents = "".to_string();
    if let Ok(result) = fs::read_to_string(patchlevel_h) {
        contents = result;
    } else if fs::metadata(&headers_path).is_err() {
        // Such a path does not exist, get out.
        return None;
    } else {
        // Try the other path
        // Sometimes we have it in a sub directory such as `python3.10`
        if let Ok(readdir) = fs::read_dir(&headers_path) {
            for path in readdir.filter_map(Result::ok).map(|e| e.path()) {
                if let Ok(metadata) = fs::metadata(&path) {
                    if metadata.is_dir() {
                        let patchlevel_h = path.join("patchlevel.h");
                        if let Ok(result) = fs::read_to_string(patchlevel_h) {
                            contents = result;
                            break;
                        }
                    }
                }
            }
        }
    }
    for line in contents.lines() {
        if let Some(captures) = VERSION.captures(line) {
            if let Some(value) = captures.get(1) {
                return Some(value.as_str().to_string());
            }
        }
    }
    None
}
