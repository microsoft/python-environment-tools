// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use glob::{glob, Pattern};
use std::{
    collections::HashSet,
    ffi::OsString,
    fmt, io,
    path::{Component, Path, PathBuf},
};

/// Characters that indicate a path contains glob pattern metacharacters.
const GLOB_METACHARACTERS: &[char] = &['*', '?', '[', ']'];

/// Checks whether a path string contains glob metacharacters or brace expansion.
///
/// # Examples
/// - `"/home/user/*"` → `true`
/// - `"/home/user/envs"` → `false`
/// - `"**/*.py"` → `true`
/// - `"/home/user/[abc]"` → `true`
/// - `"./**/{bin,Scripts}/python"` → `true`
pub fn is_glob_pattern(path: &str) -> bool {
    path.contains(GLOB_METACHARACTERS) || has_brace_pattern(path)
}

/// Returns true when a glob can traverse an unbounded number of path components.
pub fn is_recursive_glob_pattern(path: &str) -> bool {
    expand_braces(path)
        .iter()
        .any(|pattern| pattern.split(['/', '\\']).any(|segment| segment == "**"))
}

/// Checks if a string contains a valid brace expansion pattern `{a,b}`.
/// Requires an opening `{`, at least one `,`, and a closing `}`.
fn has_brace_pattern(path: &str) -> bool {
    let mut remaining = path;
    while let Some(open) = remaining.find('{') {
        let after_open = &remaining[open..];
        if let Some(close_offset) = after_open.find('}') {
            if after_open[..close_offset].contains(',') {
                return true;
            }
            remaining = &after_open[close_offset + 1..];
        } else {
            break;
        }
    }
    false
}

/// Maximum number of patterns produced by brace expansion.
/// Guards against exponential blowup from deeply nested or many brace groups.
const MAX_BRACE_EXPANSIONS: usize = 1024;
const MAX_BRACE_EXPANSION_STEPS: usize = 10_000;

/// Default limits used by JSON-RPC path expansion.
pub const DEFAULT_GLOB_EXPANSION_LIMITS: GlobExpansionLimits = GlobExpansionLimits {
    max_patterns: MAX_BRACE_EXPANSIONS,
    max_candidates: 10_000,
};

#[derive(Debug, Clone, Copy)]
pub struct GlobExpansionLimits {
    pub max_patterns: usize,
    pub max_candidates: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobExpansionError {
    PatternLimitExceeded { limit: usize },
    BraceWorkLimitExceeded { limit: usize },
    CandidateLimitExceeded { limit: usize },
    InvalidPattern { pattern: String, message: String },
    Traversal { pattern: String, message: String },
}

impl fmt::Display for GlobExpansionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PatternLimitExceeded { limit } => write!(
                formatter,
                "glob expansion exceeded the limit of {limit} distinct patterns"
            ),
            Self::BraceWorkLimitExceeded { limit } => write!(
                formatter,
                "brace expansion exceeded the limit of {limit} intermediate patterns"
            ),
            Self::CandidateLimitExceeded { limit } => write!(
                formatter,
                "glob expansion exceeded the limit of {limit} filesystem candidates"
            ),
            Self::InvalidPattern { pattern, message } => {
                write!(formatter, "invalid glob pattern '{pattern}': {message}")
            }
            Self::Traversal { pattern, message } => {
                write!(
                    formatter,
                    "failed to traverse glob pattern '{pattern}': {message}"
                )
            }
        }
    }
}

impl std::error::Error for GlobExpansionError {}

/// Expands brace expressions in a pattern string.
///
/// Handles patterns like `{a,b}` which expand to multiple strings.
/// Supports multiple brace groups and empty alternatives (e.g., `{,.exe}`).
/// Nested braces are not supported.
/// Expansion is capped at [`MAX_BRACE_EXPANSIONS`] patterns.
///
/// # Examples
/// - `"{bin,Scripts}/python"` → `["bin/python", "Scripts/python"]`
/// - `"python{,.exe}"` → `["python", "python.exe"]`
/// - `"{a,b}/{c,d}"` → `["a/c", "a/d", "b/c", "b/d"]`
fn expand_braces(pattern: &str) -> Vec<String> {
    let mut results = Vec::new();
    expand_braces_inner(pattern, &mut results);
    results
}

fn expand_braces_inner(pattern: &str, results: &mut Vec<String>) {
    let mut pending = vec![pattern.to_string()];
    let mut steps = 0;
    while let Some(pattern) = pending.pop() {
        if results.len() == MAX_BRACE_EXPANSIONS || steps == MAX_BRACE_EXPANSION_STEPS {
            log::warn!("Brace expansion exceeded its pattern/work limit, truncating '{pattern}'");
            return;
        }
        steps += 1;
        let group = pattern
            .find('{')
            .and_then(|open| pattern[open..].find('}').map(|close| (open, open + close)));
        if let Some((open, close)) = group {
            for alternative in pattern[open + 1..close].split(',').rev() {
                pending.push(format!(
                    "{}{alternative}{}",
                    &pattern[..open],
                    &pattern[close + 1..]
                ));
            }
        } else {
            results.push(pattern);
        }
    }
}

fn expand_braces_bounded(pattern: &str, limit: usize) -> Result<Vec<String>, GlobExpansionError> {
    let mut pending = vec![pattern.to_string()];
    let mut steps = 0;
    loop {
        let mut next = Vec::new();
        let mut unique = HashSet::new();
        let mut expanded = false;
        for pattern in pending {
            if steps == MAX_BRACE_EXPANSION_STEPS {
                return Err(GlobExpansionError::BraceWorkLimitExceeded {
                    limit: MAX_BRACE_EXPANSION_STEPS,
                });
            }
            steps += 1;
            let group = pattern
                .find('{')
                .and_then(|open| pattern[open..].find('}').map(|close| (open, open + close)));
            if let Some((open, close)) = group {
                expanded = true;
                for alternative in pattern[open + 1..close].split(',') {
                    if steps == MAX_BRACE_EXPANSION_STEPS {
                        return Err(GlobExpansionError::BraceWorkLimitExceeded {
                            limit: MAX_BRACE_EXPANSION_STEPS,
                        });
                    }
                    steps += 1;
                    let variant =
                        format!("{}{alternative}{}", &pattern[..open], &pattern[close + 1..]);
                    if unique.insert(variant.clone()) {
                        if next.len() == limit {
                            return Err(GlobExpansionError::PatternLimitExceeded { limit });
                        }
                        next.push(variant);
                    }
                }
            } else if unique.insert(pattern.clone()) {
                if next.len() == limit {
                    return Err(GlobExpansionError::PatternLimitExceeded { limit });
                }
                next.push(pattern);
            }
        }
        if !expanded {
            return Ok(next);
        }
        pending = next;
    }
}

#[derive(Debug)]
enum BoundedGlobComponent {
    Literal(OsString),
    Pattern(String, Pattern),
    Recursive,
}

fn increment_candidates(
    candidates_seen: &mut usize,
    limit: usize,
) -> Result<(), GlobExpansionError> {
    *candidates_seen += 1;
    if *candidates_seen > limit {
        return Err(GlobExpansionError::CandidateLimitExceeded { limit });
    }
    Ok(())
}

fn traversal_error(pattern: &str, error: io::Error) -> GlobExpansionError {
    GlobExpansionError::Traversal {
        pattern: pattern.to_string(),
        message: error.to_string(),
    }
}

fn optional_metadata(
    result: io::Result<std::fs::Metadata>,
    pattern: &str,
) -> Result<Option<std::fs::Metadata>, GlobExpansionError> {
    match result {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(traversal_error(pattern, error)),
    }
}

fn path_is_directory(path: &Path, pattern: &str) -> Result<bool, GlobExpansionError> {
    Ok(optional_metadata(std::fs::metadata(path), pattern)?
        .is_some_and(|metadata| metadata.is_dir()))
}

fn read_directory_bounded(
    directory: &Path,
    pattern: &str,
    candidates_seen: &mut usize,
    limit: usize,
) -> Result<Vec<std::fs::DirEntry>, GlobExpansionError> {
    let filesystem_directory = if directory.as_os_str().is_empty() {
        Path::new(".")
    } else {
        directory
    };
    let entries = match std::fs::read_dir(filesystem_directory) {
        Ok(entries) => entries,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(Vec::new());
        }
        Err(error) => return Err(traversal_error(pattern, error)),
    };
    let mut result = Vec::new();
    for entry in entries {
        increment_candidates(candidates_seen, limit)?;
        result.push(entry.map_err(|error| traversal_error(pattern, error))?);
    }
    result.sort_by_key(|entry| entry.file_name());
    Ok(result)
}

fn walk_bounded_glob(
    base: PathBuf,
    components: &[BoundedGlobComponent],
    require_directory: bool,
    pattern: &str,
    candidates_seen: &mut usize,
    candidate_limit: usize,
) -> Result<Vec<PathBuf>, GlobExpansionError> {
    let mut pending = vec![(base, 0)];
    let mut visited = HashSet::new();
    let mut results = Vec::new();
    while let Some((base, index)) = pending.pop() {
        if !visited.insert((base.clone(), index)) {
            continue;
        }
        let Some(component) = components.get(index) else {
            if let Some(metadata) = optional_metadata(std::fs::metadata(&base), pattern)? {
                if !require_directory || metadata.is_dir() {
                    results.push(base);
                }
            }
            continue;
        };
        let is_last = index + 1 == components.len();
        match component {
            BoundedGlobComponent::Literal(component) => {
                increment_candidates(candidates_seen, candidate_limit)?;
                let path = base.join(component);
                let metadata = match std::fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                        ) =>
                    {
                        continue
                    }
                    Err(error) => return Err(traversal_error(pattern, error)),
                };
                let is_directory = metadata.is_dir()
                    || (metadata.is_symlink() && path_is_directory(&path, pattern)?);
                if is_last && (!require_directory || is_directory) {
                    results.push(path);
                } else if is_directory {
                    pending.push((path, index + 1));
                }
            }
            BoundedGlobComponent::Pattern(text, matcher) => {
                if text.starts_with('.') {
                    for name in [".", ".."] {
                        if matcher.matches(name) {
                            increment_candidates(candidates_seen, candidate_limit)?;
                            let path = base.join(name);
                            if is_last && (!require_directory || path_is_directory(&path, pattern)?)
                            {
                                results.push(path);
                            } else if path_is_directory(&path, pattern)? {
                                pending.push((path, index + 1));
                            }
                        }
                    }
                }
                for entry in
                    read_directory_bounded(&base, pattern, candidates_seen, candidate_limit)?
                {
                    let name = entry.file_name();
                    // The glob crate skips non-UTF-8 names during wildcard matching.
                    if !name.to_str().is_some_and(|name| matcher.matches(name)) {
                        continue;
                    }
                    let path = base.join(name);
                    if is_last && (!require_directory || path_is_directory(&path, pattern)?) {
                        results.push(path);
                    } else if path_is_directory(&path, pattern)? {
                        pending.push((path, index + 1));
                    }
                }
            }
            BoundedGlobComponent::Recursive => {
                if !is_last {
                    pending.push((base.clone(), index + 1));
                }
                for entry in
                    read_directory_bounded(&base, pattern, candidates_seen, candidate_limit)?
                {
                    let path = base.join(entry.file_name());
                    if path_is_directory(&path, pattern)? {
                        if is_last {
                            results.push(path.clone());
                        }
                        pending.push((path, index));
                    }
                }
            }
        }
    }
    results.sort();
    results.dedup();
    Ok(results)
}

fn expand_filesystem_pattern_bounded(
    pattern: &str,
    candidates_seen: &mut usize,
    candidate_limit: usize,
) -> Result<Vec<PathBuf>, GlobExpansionError> {
    let require_directory = pattern
        .chars()
        .next_back()
        .is_some_and(std::path::is_separator);
    let mut base = PathBuf::new();
    let mut components = Vec::new();
    for component in Path::new(pattern).components() {
        match component {
            Component::Prefix(_) | Component::RootDir => base.push(component.as_os_str()),
            Component::CurDir | Component::ParentDir => {
                components.push(BoundedGlobComponent::Literal(
                    component.as_os_str().to_owned(),
                ));
            }
            Component::Normal(component) => {
                let component_text = component.to_string_lossy();
                if component_text == "**" {
                    if !matches!(components.last(), Some(BoundedGlobComponent::Recursive)) {
                        components.push(BoundedGlobComponent::Recursive);
                    }
                } else if component_text.contains(GLOB_METACHARACTERS) {
                    let component_pattern = Pattern::new(&component_text).map_err(|error| {
                        GlobExpansionError::InvalidPattern {
                            pattern: pattern.to_string(),
                            message: error.to_string(),
                        }
                    })?;
                    components.push(BoundedGlobComponent::Pattern(
                        component_text.into_owned(),
                        component_pattern,
                    ));
                } else {
                    components.push(BoundedGlobComponent::Literal(component.to_owned()));
                }
            }
        }
    }
    walk_bounded_glob(
        base,
        &components,
        require_directory,
        pattern,
        candidates_seen,
        candidate_limit,
    )
}

/// Expands path patterns with explicit limits and errors.
///
/// Input patterns and brace-expanded variants are deduplicated before filesystem
/// traversal. The candidate limit is checked between directory entries; it cannot
/// interrupt an operating-system filesystem call already in progress.
pub fn expand_glob_patterns_bounded(
    paths: &[PathBuf],
    limits: GlobExpansionLimits,
) -> Result<Vec<PathBuf>, GlobExpansionError> {
    let mut input_patterns = HashSet::new();
    let mut expanded_patterns = HashSet::new();
    let mut patterns = Vec::new();

    for path in paths {
        let pattern = path.to_string_lossy().into_owned();
        if !input_patterns.insert(pattern.clone()) {
            continue;
        }
        let variants = if is_glob_pattern(&pattern) {
            expand_braces_bounded(&pattern, limits.max_patterns)?
        } else {
            vec![pattern]
        };
        for variant in variants {
            if expanded_patterns.insert(variant.clone()) {
                if patterns.len() == limits.max_patterns {
                    return Err(GlobExpansionError::PatternLimitExceeded {
                        limit: limits.max_patterns,
                    });
                }
                patterns.push(variant);
            }
        }
    }

    let mut candidates_seen = 0usize;
    let mut unique_results = HashSet::new();
    let mut results = Vec::new();
    for pattern in patterns {
        if !pattern.contains(GLOB_METACHARACTERS) {
            increment_candidates(&mut candidates_seen, limits.max_candidates)?;
            let path = PathBuf::from(pattern);
            if unique_results.insert(path.clone()) {
                results.push(path);
            }
            continue;
        }

        for path in expand_filesystem_pattern_bounded(
            &pattern,
            &mut candidates_seen,
            limits.max_candidates,
        )? {
            if unique_results.insert(path.clone()) {
                results.push(path);
            }
        }
    }
    Ok(results)
}

/// Expands a single glob pattern to matching paths.
///
/// Supports brace expansion (e.g., `{bin,Scripts}`) in addition to standard
/// glob metacharacters (`*`, `?`, `[...]`). Brace groups are expanded first,
/// then each resulting pattern is matched against the filesystem.
///
/// If the path does not contain glob metacharacters or braces, returns it
/// unchanged (to let downstream code handle non-existent paths).
///
/// # Examples
/// - `"/home/user/envs"` → `["/home/user/envs"]`
/// - `"/home/user/*/venv"` → `["/home/user/project1/venv", "/home/user/project2/venv"]`
/// - `"**/.venv"` → All `.venv` directories recursively
/// - `"./**/{bin,Scripts}/python"` → Python executables in bin or Scripts dirs
pub fn expand_glob_pattern(pattern: &str) -> Vec<PathBuf> {
    if !is_glob_pattern(pattern) {
        // Not a glob pattern, return as-is
        return vec![PathBuf::from(pattern)];
    }

    // Expand brace groups first, then glob each resulting pattern
    let patterns = expand_braces(pattern);
    let mut result = Vec::new();
    for pat in &patterns {
        if !pat.contains(GLOB_METACHARACTERS) {
            // After brace expansion this variant has no glob metacharacters;
            // return as a literal path (same behavior as a non-glob input).
            result.push(PathBuf::from(pat));
            continue;
        }
        log::trace!("Expanding glob pattern '{}'", pat);
        let start = std::time::Instant::now();
        match glob(pat) {
            Ok(paths) => {
                let mut count: usize = 0;
                for entry in paths {
                    match entry {
                        Ok(path) => {
                            count += 1;
                            if count.is_multiple_of(100) {
                                log::trace!(
                                    "Glob '{}': found {} matches so far ({:?} elapsed)",
                                    pat,
                                    count,
                                    start.elapsed()
                                );
                            }
                            result.push(path);
                        }
                        Err(e) => {
                            log::debug!("Failed to read glob entry: {}", e);
                        }
                    }
                }
                log::trace!(
                    "Glob '{}': completed with {} matches in {:?}",
                    pat,
                    count,
                    start.elapsed()
                );
            }
            Err(e) => {
                log::warn!("Invalid glob pattern '{}': {}", pat, e);
            }
        }
    }
    if result.is_empty() {
        log::debug!("Glob pattern '{}' matched no paths", pattern);
    }
    result
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

    fn bounded_limits(max_patterns: usize, max_candidates: usize) -> GlobExpansionLimits {
        GlobExpansionLimits {
            max_patterns,
            max_candidates,
        }
    }

    fn assert_bounded_matches_legacy(root: &Path, suffix: &str) {
        let pattern = format!("{}{}{}", root.display(), std::path::MAIN_SEPARATOR, suffix);
        let mut legacy = expand_glob_pattern(&pattern);
        let mut bounded =
            expand_glob_patterns_bounded(&[PathBuf::from(&pattern)], bounded_limits(16, 1024))
                .unwrap();
        legacy.sort();
        bounded.sort();
        assert_eq!(bounded, legacy, "pattern: {pattern}");
    }

    #[test]
    fn bounded_metadata_errors_are_not_partial_success() {
        for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
            let result = optional_metadata(
                Err(io::Error::new(kind, "injected metadata failure")),
                "root/**",
            );
            assert!(
                matches!(result, Err(GlobExpansionError::Traversal { pattern, .. }) if pattern == "root/**")
            );
        }
        for kind in [io::ErrorKind::NotFound, io::ErrorKind::NotADirectory] {
            assert!(optional_metadata(Err(io::Error::from(kind)), "root/**")
                .unwrap()
                .is_none());
        }
        let temp = tempfile::tempdir().unwrap();
        assert!(path_is_directory(temp.path(), "root/**").unwrap());
        let file = temp.path().join("file");
        fs::write(&file, "fixture").unwrap();
        assert!(!path_is_directory(&file, "root/**").unwrap());
        assert!(!path_is_directory(&temp.path().join("missing"), "root/**").unwrap());
    }

    #[test]
    fn bounded_expansion_matches_legacy_patterns() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("EnvOne/nested")).unwrap();
        fs::create_dir_all(temp.path().join("Other")).unwrap();
        fs::create_dir_all(temp.path().join(".hidden")).unwrap();
        fs::write(temp.path().join("EnvOne/python.exe"), "").unwrap();
        fs::write(temp.path().join(".hidden/python.exe"), "").unwrap();
        fs::write(temp.path().join("root-file"), "").unwrap();
        fs::write(temp.path().join("prefixone"), "").unwrap();
        fs::write(temp.path().join("prefix"), "").unwrap();
        fs::write(temp.path().join("prefix{unmatched"), "").unwrap();

        for pattern in [
            "**",
            &format!("**{}", std::path::MAIN_SEPARATOR),
            &format!("*{}", std::path::MAIN_SEPARATOR),
            "Env*",
            "env*",
            "{EnvOne,Other}",
            "EnvOne",
            "*/python.exe",
            "**/python.exe",
            ".*",
            ".*/python.exe",
            "prefix{one}*",
            "prefix{}*",
            "prefix{unmatched*",
            "prefix{one,two}*",
            "**/**/python.exe",
            "**/EnvOne/**",
            "EnvOne/../*",
        ] {
            assert_bounded_matches_legacy(temp.path(), pattern);
        }

        assert!(expand_glob_patterns_bounded(
            &[temp.path().join("missing*")],
            bounded_limits(2, 32),
        )
        .unwrap()
        .is_empty());
        assert_eq!(
            expand_glob_patterns_bounded(
                &[
                    PathBuf::from("literal{brace}"),
                    PathBuf::from("{one,one,one}")
                ],
                bounded_limits(2, 2),
            )
            .unwrap(),
            vec![PathBuf::from("literal{brace}"), PathBuf::from("one")]
        );
    }

    #[test]
    fn bounded_expansion_matches_legacy_combinations() {
        let temp = tempfile::tempdir().unwrap();
        for directory in ["alpha", "alpha/nested", "beta", ".hidden"] {
            fs::create_dir_all(temp.path().join(directory)).unwrap();
            fs::write(temp.path().join(directory).join("python.exe"), "").unwrap();
        }
        for prefix in ["*", "**", "a*", "alpha", "{alpha,beta}", ".hidden", "[ab]*"] {
            for suffix in [
                "*",
                "**",
                "**/",
                "*/",
                "python.exe",
                "**/python.exe",
                "missing",
            ] {
                assert_bounded_matches_legacy(temp.path(), &format!("{prefix}/{suffix}"));
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn bounded_expansion_matches_broken_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("env")).unwrap();
        std::os::unix::fs::symlink("missing", temp.path().join("env/python")).unwrap();
        assert_bounded_matches_legacy(temp.path(), "*/python");
        assert_bounded_matches_legacy(temp.path(), "*/*");
    }

    #[cfg(windows)]
    #[test]
    fn bounded_expansion_matches_legacy_windows_wildcard_case() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("EnvOne")).unwrap();

        assert_bounded_matches_legacy(temp.path(), "Env*");
        assert_bounded_matches_legacy(temp.path(), "env*");
        assert_ne!(
            expand_glob_pattern(&temp.path().join("Env*").to_string_lossy()),
            expand_glob_pattern(&temp.path().join("env*").to_string_lossy())
        );
    }

    #[test]
    fn bounded_braces_deduplicate_before_cartesian_expansion() {
        assert_eq!(
            expand_braces_bounded(&"{a,a}".repeat(64), 1).unwrap(),
            vec!["a".repeat(64)]
        );
        assert!(matches!(
            expand_braces_bounded(&"{a}".repeat(MAX_BRACE_EXPANSION_STEPS), 1),
            Err(GlobExpansionError::BraceWorkLimitExceeded { .. })
        ));
    }

    #[test]
    fn duplicate_brace_alternatives_count_toward_work_limit() {
        let within_limit = format!("{{{}}}", vec!["a"; MAX_BRACE_EXPANSION_STEPS - 2].join(","));
        assert_eq!(expand_braces_bounded(&within_limit, 1).unwrap(), vec!["a"]);

        let over_limit = format!("{{{}}}", vec!["a"; MAX_BRACE_EXPANSION_STEPS].join(","));
        assert_eq!(
            expand_braces_bounded(&over_limit, 1).unwrap_err(),
            GlobExpansionError::BraceWorkLimitExceeded {
                limit: MAX_BRACE_EXPANSION_STEPS
            },
        );
        assert_eq!(
            expand_glob_patterns_bounded(&[PathBuf::from(over_limit)], bounded_limits(1, 1))
                .unwrap_err(),
            GlobExpansionError::BraceWorkLimitExceeded {
                limit: MAX_BRACE_EXPANSION_STEPS
            },
        );
    }

    #[test]
    fn bounded_expansion_reports_malformed_and_limited_patterns() {
        assert!(matches!(
            expand_glob_patterns_bounded(&[PathBuf::from("malformed[")], bounded_limits(2, 2),),
            Err(GlobExpansionError::InvalidPattern { .. })
        ));
        assert_eq!(
            expand_glob_patterns_bounded(
                &[PathBuf::from("{one,two,three}")],
                bounded_limits(2, 10),
            )
            .unwrap_err(),
            GlobExpansionError::PatternLimitExceeded { limit: 2 }
        );

        let temp = tempfile::tempdir().unwrap();
        for name in ["one", "two", "three"] {
            fs::write(temp.path().join(name), "").unwrap();
        }
        assert_eq!(
            expand_glob_patterns_bounded(&[temp.path().join("*")], bounded_limits(2, 2))
                .unwrap_err(),
            GlobExpansionError::CandidateLimitExceeded { limit: 2 }
        );

        let recursive = tempfile::tempdir().unwrap();
        fs::create_dir_all(recursive.path().join("alpha/nested")).unwrap();
        fs::create_dir_all(recursive.path().join("beta/nested")).unwrap();
        assert_eq!(
            expand_glob_patterns_bounded(
                &[recursive.path().join("**/missing")],
                bounded_limits(2, 2),
            )
            .unwrap_err(),
            GlobExpansionError::CandidateLimitExceeded { limit: 2 }
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_expansion_preserves_symlink_spelling() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("target")).unwrap();
        fs::write(temp.path().join("target/python"), "").unwrap();
        symlink(temp.path().join("target"), temp.path().join("link")).unwrap();
        assert_eq!(
            expand_glob_patterns_bounded(&[temp.path().join("link/*")], bounded_limits(2, 32),)
                .unwrap(),
            vec![temp.path().join("link/python")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn bounded_expansion_treats_unix_backslash_as_filename_data() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("name\\"), "").unwrap();
        assert_bounded_matches_legacy(temp.path(), "name*\\");
    }

    #[test]
    fn bounded_expansion_enforces_exact_candidate_budget() {
        let paths = [PathBuf::from("one"), PathBuf::from("two")];
        assert_eq!(
            expand_glob_patterns_bounded(&paths, bounded_limits(2, 2)).unwrap(),
            paths
        );
        assert_eq!(
            expand_glob_patterns_bounded(&paths, bounded_limits(2, 1)).unwrap_err(),
            GlobExpansionError::CandidateLimitExceeded { limit: 1 },
        );
    }

    #[cfg(windows)]
    #[test]
    fn bounded_expansion_preserves_junction_spelling() {
        use std::process::Command;

        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("target")).unwrap();
        fs::write(temp.path().join("target/python.exe"), "").unwrap();
        let link = temp.path().join("link");
        let target = temp.path().join("target");
        let status = Command::new("cmd.exe")
            .args([
                "/C",
                "mklink",
                "/J",
                &link.to_string_lossy(),
                &target.to_string_lossy(),
            ])
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            expand_glob_patterns_bounded(&[temp.path().join("link/*")], bounded_limits(2, 32),)
                .unwrap(),
            vec![temp.path().join("link/python.exe")]
        );
        fs::remove_dir(link).unwrap();
    }

    #[test]
    fn test_is_glob_pattern_with_asterisk() {
        assert!(is_glob_pattern("/home/user/*"));
        assert!(is_glob_pattern("**/*.py"));
        assert!(is_glob_pattern("*.txt"));
    }

    #[test]
    fn test_is_recursive_glob_pattern() {
        assert!(is_recursive_glob_pattern("**/.venv"));
        assert!(is_recursive_glob_pattern("/home/user/**/venv"));
        assert!(!is_recursive_glob_pattern(".venv"));
        assert!(!is_recursive_glob_pattern("*/.venv"));
        assert!(!is_recursive_glob_pattern("foo**bar/.venv"));
        assert!(is_recursive_glob_pattern("C:\\workspace\\**\\.venv"));
        assert!(is_recursive_glob_pattern("{foo,**}/.venv"));
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

    #[test]
    fn test_expand_braces_no_braces() {
        assert_eq!(expand_braces("no_braces"), vec!["no_braces"]);
        assert_eq!(expand_braces("/usr/bin/python"), vec!["/usr/bin/python"]);
    }

    #[test]
    fn test_expand_braces_single_group() {
        let mut result = expand_braces("{bin,Scripts}/python");
        result.sort();
        assert_eq!(result, vec!["Scripts/python", "bin/python"]);
    }

    #[test]
    fn test_expand_braces_empty_alternative() {
        let mut result = expand_braces("python{,.exe}");
        result.sort();
        assert_eq!(result, vec!["python", "python.exe"]);
    }

    #[test]
    fn test_expand_braces_multiple_groups() {
        let mut result = expand_braces("{a,b}/{c,d}");
        result.sort();
        assert_eq!(result, vec!["a/c", "a/d", "b/c", "b/d"]);
    }

    #[test]
    fn test_expand_braces_unmatched_brace() {
        assert_eq!(expand_braces("{unmatched"), vec!["{unmatched"]);
    }

    #[test]
    fn test_expand_braces_real_pattern() {
        let mut result = expand_braces("./**/{bin,Scripts}/python{,.exe}");
        result.sort();
        assert_eq!(
            result,
            vec![
                "./**/Scripts/python",
                "./**/Scripts/python.exe",
                "./**/bin/python",
                "./**/bin/python.exe",
            ]
        );
    }

    #[test]
    fn test_is_glob_pattern_with_braces() {
        assert!(is_glob_pattern("{bin,Scripts}/python"));
        assert!(is_glob_pattern("python{,.exe}"));
        assert!(is_glob_pattern("./**/{bin,Scripts}/python{,.exe}"));
    }

    #[test]
    fn test_is_glob_pattern_lone_brace_not_detected() {
        // A lone `{` without matching `}` or without comma is not a brace pattern
        assert!(!is_glob_pattern("/home/user/my{project"));
        assert!(!is_glob_pattern("{nocomma}"));
        // But a proper `{a,b}` pair is detected even without glob metacharacters
        assert!(is_glob_pattern("{a,b}"));
    }

    #[test]
    fn test_is_glob_pattern_brace_after_non_brace() {
        // A valid brace group after a non-expansion group should still be detected
        assert!(is_glob_pattern("{nocomma}/path/{a,b}"));
    }

    #[test]
    fn test_expand_braces_empty_braces() {
        // `{}` has no comma, so expand_braces treats it as a single empty alternative
        assert_eq!(expand_braces("prefix{}suffix"), vec!["prefixsuffix"]);
    }

    #[test]
    fn test_expand_braces_single_alternative() {
        assert_eq!(expand_braces("{a}"), vec!["a"]);
    }

    #[test]
    fn test_expand_braces_capped() {
        // Build a pattern that would produce 2^20 = 1M+ expansions without cap
        let pattern = "{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}";
        let result = expand_braces(pattern);
        assert_eq!(result.len(), MAX_BRACE_EXPANSIONS);
    }

    #[test]
    fn test_expand_glob_pattern_with_braces() {
        // Create temp directories with bin and Scripts subdirs
        let temp_dir = std::env::temp_dir().join("pet_glob_test_braces");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("env1/bin")).unwrap();
        fs::create_dir_all(temp_dir.join("env2/Scripts")).unwrap();
        fs::write(temp_dir.join("env1/bin/python"), "").unwrap();
        fs::write(temp_dir.join("env2/Scripts/python.exe"), "").unwrap();

        let pattern = format!(
            "{}/**/{{bin,Scripts}}/python{{,.exe}}",
            temp_dir.to_string_lossy()
        );
        let result = expand_glob_pattern(&pattern);

        assert_eq!(result.len(), 2);
        assert!(result
            .iter()
            .any(|p| p.ends_with("bin/python") || p.ends_with("bin\\python")));
        assert!(result
            .iter()
            .any(|p| p.ends_with("Scripts/python.exe") || p.ends_with("Scripts\\python.exe")));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    // ── expand_braces: additional edge cases ──

    #[test]
    fn test_expand_braces_three_alternatives() {
        let mut result = expand_braces("{a,b,c}");
        result.sort();
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_expand_braces_preserves_surrounding_text() {
        let mut result = expand_braces("prefix/{a,b}/suffix");
        result.sort();
        assert_eq!(result, vec!["prefix/a/suffix", "prefix/b/suffix"]);
    }

    #[test]
    fn test_expand_braces_adjacent_groups() {
        // Two brace groups with no separator: {a,b}{c,d} → ac, ad, bc, bd
        let mut result = expand_braces("{a,b}{c,d}");
        result.sort();
        assert_eq!(result, vec!["ac", "ad", "bc", "bd"]);
    }

    #[test]
    fn test_expand_braces_with_dots_and_extensions() {
        let mut result = expand_braces("file{.txt,.md,.rs}");
        result.sort();
        assert_eq!(result, vec!["file.md", "file.rs", "file.txt"]);
    }

    #[test]
    fn test_expand_braces_empty_in_middle() {
        // {a,,b} should produce "a", "", "b" (prefix/suffix applied)
        let mut result = expand_braces("x{a,,b}y");
        result.sort();
        assert_eq!(result, vec!["xay", "xby", "xy"]);
    }

    #[test]
    fn test_expand_braces_single_char_alternatives() {
        let mut result = expand_braces("{x,y,z}");
        result.sort();
        assert_eq!(result, vec!["x", "y", "z"]);
    }

    #[test]
    fn test_expand_braces_path_separators() {
        let mut result = expand_braces("/home/{user1,user2}/.local/bin");
        result.sort();
        assert_eq!(
            result,
            vec!["/home/user1/.local/bin", "/home/user2/.local/bin",]
        );
    }

    #[test]
    fn test_expand_braces_windows_style_paths() {
        let mut result = expand_braces("C:\\envs\\{venv1,venv2}\\{Scripts,bin}\\python.exe");
        result.sort();
        assert_eq!(
            result,
            vec![
                "C:\\envs\\venv1\\Scripts\\python.exe",
                "C:\\envs\\venv1\\bin\\python.exe",
                "C:\\envs\\venv2\\Scripts\\python.exe",
                "C:\\envs\\venv2\\bin\\python.exe",
            ]
        );
    }

    #[test]
    fn test_expand_braces_only_empty_alternatives() {
        // {,} should produce two empty strings → prefix+suffix twice
        let result = expand_braces("a{,}b");
        assert_eq!(result, vec!["ab", "ab"]);
    }

    #[test]
    fn test_expand_braces_mixed_with_glob_chars() {
        // Braces with glob metacharacters inside alternatives
        let mut result = expand_braces("{*.py,*.rs}");
        result.sort();
        assert_eq!(result, vec!["*.py", "*.rs"]);
    }

    // ── is_glob_pattern: additional edge cases ──

    #[test]
    fn test_is_glob_not_glob_empty_string() {
        assert!(!is_glob_pattern(""));
    }

    #[test]
    fn test_is_glob_brace_no_close() {
        assert!(!is_glob_pattern("path/{open,but,no,close"));
    }

    #[test]
    fn test_is_glob_close_before_open() {
        // Stray `}` before any `{` — no valid brace pattern at all
        assert!(!is_glob_pattern("path}/no/braces"));
        // But a stray `}` followed by a valid `{a,b}` IS a brace pattern
        assert!(is_glob_pattern("path}/then/{a,b}"));
    }

    #[test]
    fn test_is_glob_multiple_groups_only_second_valid() {
        assert!(is_glob_pattern("{single}/{a,b}"));
    }

    // ── expand_braces: cap behavior ──

    #[test]
    fn test_expand_braces_cap_stops_at_limit() {
        // 3^7 = 2187 > 1024, should be capped
        let pattern = "{a,b,c}/{a,b,c}/{a,b,c}/{a,b,c}/{a,b,c}/{a,b,c}/{a,b,c}";
        let result = expand_braces(pattern);
        assert_eq!(result.len(), MAX_BRACE_EXPANSIONS);
        // All results should be valid path-like strings
        assert!(result.iter().all(|s| s.contains('/')));
    }

    #[test]
    fn test_expand_braces_just_under_cap() {
        // 2^10 = 1024, exactly at the cap
        let pattern = "{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}";
        let result = expand_braces(pattern);
        assert_eq!(result.len(), MAX_BRACE_EXPANSIONS);
    }

    #[test]
    fn test_expand_braces_well_under_cap() {
        // 2^3 = 8, well under cap
        let pattern = "{a,b}/{a,b}/{a,b}";
        let result = expand_braces(pattern);
        assert_eq!(result.len(), 8);
    }

    // ── Filesystem: brace expansion + glob integration ──

    #[test]
    fn test_expand_glob_braces_with_nested_dirs() {
        let temp_dir = std::env::temp_dir().join("pet_glob_test_nested_braces");
        let _ = fs::remove_dir_all(&temp_dir);

        // Simulate a workspace with multiple envs, each having bin or Scripts
        fs::create_dir_all(temp_dir.join("proj1/.venv/bin")).unwrap();
        fs::create_dir_all(temp_dir.join("proj2/.venv/Scripts")).unwrap();
        fs::create_dir_all(temp_dir.join("proj3/.conda/bin")).unwrap();
        fs::write(temp_dir.join("proj1/.venv/bin/python"), "").unwrap();
        fs::write(temp_dir.join("proj2/.venv/Scripts/python.exe"), "").unwrap();
        fs::write(temp_dir.join("proj3/.conda/bin/python"), "").unwrap();
        // Decoy file that should NOT match
        fs::write(temp_dir.join("proj3/.conda/bin/pip"), "").unwrap();

        let pattern = format!(
            "{}/**/{{bin,Scripts}}/python{{,.exe}}",
            temp_dir.to_string_lossy()
        );
        let result = expand_glob_pattern(&pattern);

        assert_eq!(
            result.len(),
            3,
            "Expected 3 python executables, got: {:?}",
            result
        );

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_expand_glob_braces_no_matching_alternative() {
        let temp_dir = std::env::temp_dir().join("pet_glob_test_braces_nomatch");
        let _ = fs::remove_dir_all(&temp_dir);

        // Only create bin, not Scripts
        fs::create_dir_all(temp_dir.join("env/bin")).unwrap();
        fs::write(temp_dir.join("env/bin/python"), "").unwrap();

        let pattern = format!("{}/**/{{bin,Scripts}}/python", temp_dir.to_string_lossy());
        let result = expand_glob_pattern(&pattern);

        // Only bin/python should match, Scripts/python shouldn't exist
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("python"));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_expand_glob_braces_empty_alternative_finds_both() {
        let temp_dir = std::env::temp_dir().join("pet_glob_test_braces_empty_alt");
        let _ = fs::remove_dir_all(&temp_dir);

        fs::create_dir_all(temp_dir.join("bin")).unwrap();
        fs::write(temp_dir.join("bin/python"), "").unwrap();
        fs::write(temp_dir.join("bin/python.exe"), "").unwrap();

        let pattern = format!("{}/bin/python{{,.exe}}", temp_dir.to_string_lossy());
        let result = expand_glob_pattern(&pattern);

        assert_eq!(
            result.len(),
            2,
            "Expected both python and python.exe, got: {:?}",
            result
        );

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_expand_glob_brace_only_pattern_returns_literals() {
        // A brace-only pattern (no glob metacharacters after expansion)
        // should return literal paths without filesystem validation
        let result = expand_glob_pattern("{python,python3}");
        assert_eq!(result.len(), 2);
        assert!(result.contains(&PathBuf::from("python")));
        assert!(result.contains(&PathBuf::from("python3")));
    }

    #[test]
    fn test_expand_glob_patterns_with_braces_in_list() {
        let temp_dir = std::env::temp_dir().join("pet_glob_test_patterns_list");
        let _ = fs::remove_dir_all(&temp_dir);

        fs::create_dir_all(temp_dir.join("a/bin")).unwrap();
        fs::write(temp_dir.join("a/bin/python"), "").unwrap();

        let paths = vec![
            PathBuf::from("/literal/path"),
            PathBuf::from(format!(
                "{}/**/{{bin,Scripts}}/python",
                temp_dir.to_string_lossy()
            )),
        ];
        let result = expand_glob_patterns(&paths);

        // literal + 1 glob match
        assert_eq!(result.len(), 2);
        assert!(result.contains(&PathBuf::from("/literal/path")));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    // ── Performance tests ──

    #[test]
    fn test_expand_braces_performance_many_alternatives() {
        // 100 alternatives in a single group — should be instant
        let alts: Vec<String> = (0..100).map(|i| format!("alt{i}")).collect();
        let pattern = format!("{{{}}}", alts.join(","));

        let start = std::time::Instant::now();
        let result = expand_braces(&pattern);
        let elapsed = start.elapsed();

        assert_eq!(result.len(), 100);
        assert!(
            elapsed.as_millis() < 100,
            "Expanding 100 alternatives took {:?}, expected < 100ms",
            elapsed
        );
    }

    #[test]
    fn test_expand_braces_performance_multiple_groups() {
        // 4 groups of 4 alternatives = 256 patterns
        let pattern = "{a,b,c,d}/{e,f,g,h}/{i,j,k,l}/{m,n,o,p}";

        let start = std::time::Instant::now();
        let result = expand_braces(pattern);
        let elapsed = start.elapsed();

        assert_eq!(result.len(), 256);
        assert!(
            elapsed.as_millis() < 100,
            "Expanding 4x4 groups (256 patterns) took {:?}, expected < 100ms",
            elapsed
        );
    }

    #[test]
    fn test_expand_braces_performance_cap_is_fast() {
        // Pattern that would produce 2^20 = 1M+ expansions without the cap.
        // The cap should make this complete quickly.
        let pattern = "{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}/{a,b}";

        let start = std::time::Instant::now();
        let result = expand_braces(pattern);
        let elapsed = start.elapsed();

        assert_eq!(result.len(), MAX_BRACE_EXPANSIONS);
        assert!(
            elapsed.as_millis() < 100,
            "Capped expansion (2^20 input) took {:?}, expected < 100ms",
            elapsed
        );
    }

    #[test]
    fn test_expand_glob_performance_braces_with_filesystem() {
        // Create a moderately deep directory tree and time glob with braces
        let temp_dir = std::env::temp_dir().join("pet_glob_test_perf");
        let _ = fs::remove_dir_all(&temp_dir);

        // Create 50 project dirs, each with bin/python and Scripts/python.exe
        for i in 0..50 {
            let proj = temp_dir.join(format!("project{i}/.venv"));
            fs::create_dir_all(proj.join("bin")).unwrap();
            fs::create_dir_all(proj.join("Scripts")).unwrap();
            fs::write(proj.join("bin/python"), "").unwrap();
            fs::write(proj.join("Scripts/python.exe"), "").unwrap();
        }

        let pattern = format!(
            "{}/**/{{bin,Scripts}}/python{{,.exe}}",
            temp_dir.to_string_lossy()
        );

        let start = std::time::Instant::now();
        let result = expand_glob_pattern(&pattern);
        let elapsed = start.elapsed();

        // Each project has bin/python + Scripts/python.exe = 2, * 50 projects = 100
        assert_eq!(
            result.len(),
            100,
            "Expected 100 matches, got {}: {:?}",
            result.len(),
            result
        );
        assert!(
            elapsed.as_secs() < 5,
            "Glob with braces over 50 projects took {:?}, expected < 5s",
            elapsed
        );

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_expand_glob_performance_no_braces_comparison() {
        // Same structure as above but using plain glob (no braces) for comparison
        let temp_dir = std::env::temp_dir().join("pet_glob_test_perf_no_braces");
        let _ = fs::remove_dir_all(&temp_dir);

        for i in 0..50 {
            let proj = temp_dir.join(format!("project{i}/.venv/bin"));
            fs::create_dir_all(&proj).unwrap();
            fs::write(proj.join("python"), "").unwrap();
        }

        let pattern = format!("{}/**/bin/python", temp_dir.to_string_lossy());

        let start = std::time::Instant::now();
        let result = expand_glob_pattern(&pattern);
        let elapsed = start.elapsed();

        assert_eq!(result.len(), 50);
        assert!(
            elapsed.as_secs() < 5,
            "Plain glob over 50 projects took {:?}, expected < 5s",
            elapsed
        );

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
