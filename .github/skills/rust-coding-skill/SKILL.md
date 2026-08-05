---
name: "rust-coding-skill"
description: "Use whenever editing Rust in PET to write allocation-aware, cross-platform, byte-safe code with behavior-proving tests."
user-invocable: true
---

# PET Rust Coding Skill

Use this alongside `rust-locator-patterns`. Priority order:

1. Readable code with explicit invariants
2. Correct cross-platform and concurrent behavior
3. Measured performance improvements without duplicate work

## Path Identity and Caches

Use `Path`/`PathBuf` for paths. Preserve the caller-facing path in reported values, but normalize cache and comparison keys with existing PET helpers such as `norm_case`.

A normalized key does not imply the cached value can expose the first caller's spelling:

```rust
let key = norm_case(path);
let mut cached = cache.get(&key)?.clone();
cached.prefix = Some(path.to_path_buf());
```

When adding or reviewing a path-keyed cache, check lookup, insert, remove, retain/prune, and state-sync paths. Add Windows coverage using equivalent separators or casing; do not test only the happy-path spelling.

## Byte-Safe Parsing

Never calculate byte offsets from a transformed Unicode string and apply them to the original. Unicode case conversion can change byte length.

For ASCII wire/file markers, use byte-stable ASCII-insensitive matching and checked slicing:

```rust
let start = find_ascii_case_insensitive(line, "# cmd:")? + "# cmd:".len();
let end = find_ascii_case_insensitive(line, " create -")?;
let value = line.get(start..end)?.trim();
```

Use `to_ascii_lowercase` rather than `to_lowercase` when the format is defined as ASCII. Add a non-ASCII path regression test whenever offsets are derived from textual markers.

## Hot-Path I/O and Allocations

Discovery runs frequently and in parallel. Before adding a cache, prove the repeated work and define invalidation. Within one operation, read immutable metadata once and pass borrowed snapshots through parsers.

- Prefer `&str`/`&[u8]` over cloning content between parsers.
- Prefer `rfind`/iterator operations over collecting an intermediate `Vec` just to select one item.
- Avoid `format!` and Unicode case conversion in per-line loops when ASCII matching or direct writes suffice.
- Do not claim an optimization is complete until every relevant call path is traced, including base/root environments and manager lookup.
- Do not emit the same warning, telemetry event, or report from both a pre-check and the worker path.

## Error Handling and Locks

Library code should preserve typed information where repository APIs permit it. Prefer `?`, `let-else`, and `if let` over broad fallbacks. Do not swallow filesystem errors when doing so can make stale cache data look valid.

Use contextual `expect` for poisoned locks in production code, matching the surrounding crate. Keep lock scopes short and never perform filesystem I/O or callbacks while holding a shared-state lock unless the design explicitly requires it.

## Cross-Platform Semantics

- Use `#[cfg(...)]` for platform-only code; `cfg!` does not prevent compilation.
- Avoid `canonicalize` for Windows junction identity; use PET path helpers.
- Treat both `/` and `\` as separators when parsing user patterns, but only classify `**` as recursive when it is a complete path segment. `foo**bar` is not a recursive segment.
- Preserve original user-facing paths after normalized comparisons.

## Tests Must Prove the Change

Tests should demonstrate the behavior or performance invariant, not merely execute new lines.

For optimizations, instrument the dependency boundary and assert the operation count:

```rust
let reads = Cell::new(0);
parse_with_reader(path, |_| {
    reads.set(reads.get() + 1);
    Some(history.clone())
});
assert_eq!(reads.get(), 1);
```

For parser helpers, include malformed input, non-ASCII surrounding data, and case variations. For diagnostics, test pattern classification and expansion filtering separately. Keep temp paths unique with `tempfile` or process/counter-based names.

Before every Rust commit, run the targeted tests plus `scripts/rust-precommit.ps1` (or `.sh`). Do not suppress Clippy warnings to land a change.