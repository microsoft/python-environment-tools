// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    fs,
    path::{Path, PathBuf},
};

use log::{error, trace};

pub struct PyProjectToml {
    pub name: String,
}

impl PyProjectToml {
    fn new(name: String, file: PathBuf) -> Self {
        trace!("Poetry project: {:?} with name {:?}", file, name);
        PyProjectToml { name }
    }
    pub fn find(path: &Path) -> Option<Self> {
        trace!("Finding poetry file in {:?}", path);
        parse(&path.join("pyproject.toml"))
    }
}

fn parse(file: &Path) -> Option<PyProjectToml> {
    trace!("Parsing poetry file: {:?}", file);
    match fs::read_to_string(file) {
        Ok(contents) => {
            trace!(
                "Parsed contents of poetry file: {:?} is {:?}",
                file,
                &contents
            );
        }
        Err(e) => {
            error!("Error reading poetry file: {:?}", e);
        }
    };
    let contents = fs::read_to_string(file).ok()?;
    trace!(
        "Parsed contents of poetry file: {:?} is {:?}",
        file,
        &contents
    );
    parse_contents(&contents, file)
}

fn parse_contents(contents: &str, file: &Path) -> Option<PyProjectToml> {
    trace!(
        "Parsing contents of poetry file: {:?} with contents {:?}",
        file,
        contents
    );
    match toml::from_str::<toml::Value>(contents) {
        Ok(value) => {
            let mut name = None;
            if let Some(tool) = value.get("tool") {
                if let Some(poetry) = tool.get("poetry") {
                    if let Some(name_value) = poetry.get("name") {
                        name = name_value.as_str().map(|s| s.to_string());
                    }
                }
            }
            trace!("Successfully parsed TOML value: {:?}", value);
            name.map(|name| PyProjectToml::new(name, file.into()))
        }
        Err(e) => {
            error!("Error parsing toml file: {:?}", e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn extract_name_from_pyproject_toml() {
        let cfg = r#"
[tool.poetry]
name = "poetry-demo"
version = "0.1.0"
description = ""
authors = ["User Name <bogus.user@some.email.com>"]
readme = "README.md"

[tool.poetry.dependencies]
python = "^3.12"


[build-system]
requires = ["poetry-core"]
build-backend = "poetry.core.masonry.api"
"#;
        assert_eq!(
            parse_contents(&cfg.to_string(), Path::new("pyproject.toml"))
                .unwrap()
                .name,
            "poetry-demo"
        );
    }
}
