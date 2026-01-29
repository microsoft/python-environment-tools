// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use glob::glob;
use std::path::PathBuf;

/// Characters that indicate a path contains glob pattern metacharacters.
const GLOB_METACHARACTERS: &[char] = &['*', '?', '[', ']'];

/// Checks whether a path string contains glob metacharacters.
///
/// # Examples
/// - `"/home/user/*"` → `true`
/// - `"/home/user/envs"` → `false`
/// - `"**/*.py"` → `true`
/// - `"/home/user/[abc]"` → `true`
pub fn is_glob_pattern(path: &str) -> bool {
    path.contains(GLOB_METACHARACTERS)
}

/// Expands a single glob pattern to matching paths.
///
/// If the path does not contain glob metacharacters, returns it unchanged (if it exists)
/// or as-is (to let downstream code handle non-existent paths).
///
/// If the path is a glob pattern, expands it and returns all matching paths.
/// Pattern errors and unreadable paths are logged and skipped.
///
/// # Examples
/// - `"/home/user/envs"` → `["/home/user/envs"]`
/// - `"/home/user/*/venv"` → `["/home/user/project1/venv", "/home/user/project2/venv"]`
/// - `"**/.venv"` → All `.venv` directories recursively
pub fn expand_glob_pattern(pattern: &str) -> Vec<PathBuf> {
    if !is_glob_pattern(pattern) {
        // Not a glob pattern, return as-is
        return vec![PathBuf::from(pattern)];
    }

    match glob(pattern) {
        Ok(paths) => {
            let mut result = Vec::new();
            for entry in paths {
                match entry {
                    Ok(path) => result.push(path),
                    Err(e) => {
                        log::debug!("Failed to read glob entry: {}", e);
                    }
                }
            }
            if result.is_empty() {
                log::debug!("Glob pattern '{}' matched no paths", pattern);
            }
            result
        }
        Err(e) => {
            log::warn!("Invalid glob pattern '{}': {}", pattern, e);
            Vec::new()
        }
    }
}

/// Expands a list of paths, where each path may be a glob pattern.
///
/// Non-glob paths are passed through as-is.
/// Glob patterns are expanded to all matching paths.
/// Duplicate paths are preserved (caller should deduplicate if needed).
///
/// # Examples
/// ```ignore
/// let paths = vec![
///     PathBuf::from("/home/user/project"),
///     PathBuf::from("/home/user/*/venv"),
/// ];
/// let expanded = expand_glob_patterns(&paths);
/// // expanded contains "/home/user/project" plus all matching venv dirs
/// ```
pub fn expand_glob_patterns(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for path in paths {
        let path_str = path.to_string_lossy();
        let expanded = expand_glob_pattern(&path_str);
        result.extend(expanded);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_is_glob_pattern_with_asterisk() {
        assert!(is_glob_pattern("/home/user/*"));
        assert!(is_glob_pattern("**/*.py"));
        assert!(is_glob_pattern("*.txt"));
    }

    #[test]
    fn test_is_glob_pattern_with_question_mark() {
        assert!(is_glob_pattern("/home/user/file?.txt"));
        assert!(is_glob_pattern("test?"));
    }

    #[test]
    fn test_is_glob_pattern_with_brackets() {
        assert!(is_glob_pattern("/home/user/[abc]"));
        assert!(is_glob_pattern("file[0-9].txt"));
    }

    #[test]
    fn test_is_glob_pattern_no_metacharacters() {
        assert!(!is_glob_pattern("/home/user/envs"));
        assert!(!is_glob_pattern("simple_path"));
        assert!(!is_glob_pattern("/usr/local/bin/python3"));
    }

    #[test]
    fn test_expand_non_glob_path() {
        let path = "/some/literal/path";
        let result = expand_glob_pattern(path);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], PathBuf::from(path));
    }

    #[test]
    fn test_expand_glob_pattern_no_matches() {
        let pattern = "/this/path/definitely/does/not/exist/*";
        let result = expand_glob_pattern(pattern);
        assert!(result.is_empty());
    }

    #[test]
    fn test_expand_glob_pattern_with_matches() {
        // Create temp directories for testing
        let temp_dir = std::env::temp_dir().join("pet_glob_test");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("project1")).unwrap();
        fs::create_dir_all(temp_dir.join("project2")).unwrap();
        fs::create_dir_all(temp_dir.join("other")).unwrap();

        let pattern = format!("{}/project*", temp_dir.to_string_lossy());
        let result = expand_glob_pattern(&pattern);

        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|p| p.ends_with("project1")));
        assert!(result.iter().any(|p| p.ends_with("project2")));
        assert!(!result.iter().any(|p| p.ends_with("other")));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_expand_glob_patterns_mixed() {
        let temp_dir = std::env::temp_dir().join("pet_glob_test_mixed");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("dir1")).unwrap();
        fs::create_dir_all(temp_dir.join("dir2")).unwrap();

        let paths = vec![
            PathBuf::from("/literal/path"),
            PathBuf::from(format!("{}/dir*", temp_dir.to_string_lossy())),
        ];

        let result = expand_glob_patterns(&paths);

        // Should have literal path + 2 expanded directories
        assert_eq!(result.len(), 3);
        assert!(result.contains(&PathBuf::from("/literal/path")));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_expand_glob_pattern_recursive() {
        // Create nested temp directories for testing **
        let temp_dir = std::env::temp_dir().join("pet_glob_test_recursive");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("a/b/.venv")).unwrap();
        fs::create_dir_all(temp_dir.join("c/.venv")).unwrap();
        fs::create_dir_all(temp_dir.join(".venv")).unwrap();

        let pattern = format!("{}/**/.venv", temp_dir.to_string_lossy());
        let result = expand_glob_pattern(&pattern);

        // Should find .venv at multiple levels (behavior depends on glob crate version)
        assert!(!result.is_empty());
        assert!(result.iter().all(|p| p.ends_with(".venv")));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_expand_glob_pattern_filename_patterns() {
        // Create temp files for testing filename patterns like python_* and python.*
        let temp_dir = std::env::temp_dir().join("pet_glob_test_filenames");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // Create files matching python_* pattern
        fs::write(temp_dir.join("python_foo"), "").unwrap();
        fs::write(temp_dir.join("python_bar"), "").unwrap();
        fs::write(temp_dir.join("python_3.12"), "").unwrap();
        fs::write(temp_dir.join("other_file"), "").unwrap();

        // Test python_* pattern
        let pattern = format!("{}/python_*", temp_dir.to_string_lossy());
        let result = expand_glob_pattern(&pattern);

        assert_eq!(result.len(), 3);
        assert!(result.iter().any(|p| p.ends_with("python_foo")));
        assert!(result.iter().any(|p| p.ends_with("python_bar")));
        assert!(result.iter().any(|p| p.ends_with("python_3.12")));
        assert!(!result.iter().any(|p| p.ends_with("other_file")));

        // Create files matching python.* pattern
        fs::write(temp_dir.join("python.exe"), "").unwrap();
        fs::write(temp_dir.join("python.sh"), "").unwrap();
        fs::write(temp_dir.join("pythonrc"), "").unwrap();

        // Test python.* pattern
        let pattern = format!("{}/python.*", temp_dir.to_string_lossy());
        let result = expand_glob_pattern(&pattern);

        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|p| p.ends_with("python.exe")));
        assert!(result.iter().any(|p| p.ends_with("python.sh")));
        assert!(!result.iter().any(|p| p.ends_with("pythonrc")));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
