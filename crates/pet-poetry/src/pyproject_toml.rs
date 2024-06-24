// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::{
    fs,
    path::{Path, PathBuf},
};

use log::trace;

pub struct PyProjectToml {
    pub name: String,
}

impl PyProjectToml {
    fn new(name: String, file: PathBuf) -> Self {
        trace!("Poetry project: {:?} with name {:?}", file, name);
        PyProjectToml { name }
    }
    pub fn find(path: &Path) -> Option<Self> {
        parse(&path.join("pyproject.toml"))
    }
}

fn parse(file: &Path) -> Option<PyProjectToml> {
    let contents = fs::read_to_string(file).ok()?;
    parse_contents(&contents, file)
}

fn parse_contents(contents: &str, file: &Path) -> Option<PyProjectToml> {
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
            name.map(|name| PyProjectToml::new(name, file.into()))
        }
        Err(e) => {
            eprintln!("Error parsing toml file: {:?}", e);
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
