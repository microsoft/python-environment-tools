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
    #[allow(dead_code)]
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
    // Generally the files are in Headers in windows and include in unix
    // However they can also be in Headers on Mac (command line tools python, hence make no assumptions)
    for headers_path in [path.join("Headers"), path.join("include")] {
        let patchlevel_h = headers_path.join("patchlevel.h");
        let mut contents = "".to_string();
        if let Ok(result) = fs::read_to_string(patchlevel_h) {
            contents = result;
        } else if fs::metadata(&headers_path).is_err() {
            // TODO: Remove this check, unnecessary, as we try to read the dir below.
            // Such a path does not exist, get out.
            continue;
        } else {
            // Try the other path
            // Sometimes we have it in a sub directory such as `python3.10` or `pypy3.9`
            if let Ok(readdir) = fs::read_dir(&headers_path) {
                for path in readdir
                    .filter_map(Result::ok)
                    // .filter(|d| d.file_type().is_ok_and(|f| f.is_dir()))
                    .map(|d| d.path())
                {
                    let patchlevel_h = path.join("patchlevel.h");
                    if let Ok(result) = fs::read_to_string(patchlevel_h) {
                        contents = result;
                        break;
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
    }
    None
}
