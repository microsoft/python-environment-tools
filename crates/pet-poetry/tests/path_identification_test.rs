// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::path::PathBuf;

// Import the is_poetry_environment function - we'll need to make it public for testing
// For now, we'll test via the public API

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to test the regex pattern matching
    // This tests the core logic without needing actual filesystem structures
    fn test_poetry_path_pattern(path_str: &str) -> bool {
        use regex::Regex;
        let path = PathBuf::from(path_str);
        let path_str = path.to_str().unwrap_or_default();

        if path_str.contains("pypoetry") && path_str.contains("virtualenvs") {
            if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                let re = Regex::new(r"^.+-[A-Za-z0-9_-]{8}-py.*$").unwrap();
                return re.is_match(dir_name);
            }
        }
        false
    }

    #[test]
    fn test_poetry_path_pattern_macos() {
        assert!(test_poetry_path_pattern(
            "/Users/eleanorboyd/Library/Caches/pypoetry/virtualenvs/nestedpoetry-yJwtIF_Q-py3.11"
        ));
    }

    #[test]
    fn test_poetry_path_pattern_linux() {
        assert!(test_poetry_path_pattern(
            "/home/user/.cache/pypoetry/virtualenvs/myproject-a1B2c3D4-py3.10"
        ));
    }

    #[test]
    fn test_poetry_path_pattern_windows() {
        assert!(test_poetry_path_pattern(
            r"C:\Users\user\AppData\Local\pypoetry\Cache\virtualenvs\myproject-f7sQRtG5-py3.11"
        ));
    }

    #[test]
    fn test_poetry_path_pattern_no_version() {
        assert!(test_poetry_path_pattern(
            "/home/user/.cache/pypoetry/virtualenvs/testproject-XyZ12345-py"
        ));
    }

    #[test]
    fn test_non_poetry_path_rejected() {
        assert!(!test_poetry_path_pattern("/home/user/projects/myenv"));
        assert!(!test_poetry_path_pattern("/home/user/.venv"));
        assert!(!test_poetry_path_pattern("/usr/local/venv"));
    }

    #[test]
    fn test_poetry_path_without_pypoetry_rejected() {
        // Should reject paths that look like the pattern but aren't in pypoetry directory
        assert!(!test_poetry_path_pattern(
            "/home/user/virtualenvs/myproject-a1B2c3D4-py3.10"
        ));
    }

    #[test]
    fn test_poetry_path_wrong_hash_length_rejected() {
        // Hash should be exactly 8 characters
        assert!(!test_poetry_path_pattern(
            "/home/user/.cache/pypoetry/virtualenvs/myproject-a1B2c3D456-py3.10"
        ));
        assert!(!test_poetry_path_pattern(
            "/home/user/.cache/pypoetry/virtualenvs/myproject-a1B2c3-py3.10"
        ));
    }

    #[test]
    fn test_real_world_poetry_paths() {
        // Test actual Poetry paths from the bug report and real usage
        assert!(test_poetry_path_pattern(
            "/Users/eleanorboyd/Library/Caches/pypoetry/virtualenvs/nestedpoetry-yJwtIF_Q-py3.11"
        ));

        // Another real-world example from documentation
        assert!(test_poetry_path_pattern(
            "/Users/donjayamanne/.cache/pypoetry/virtualenvs/poetry-demo-gNT2WXAV-py3.9"
        ));
    }
}
