---
name: "rust-coding-skill"
description: "Use whenever editing Rust in PET to write allocation-aware, cross-platform, byte-safe code with behavior-proving tests."
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
let marker = b"# cmd:";
let start = line
    .as_bytes()
    .windows(marker.len())
    .position(|window| window.eq_ignore_ascii_case(marker))?
    + marker.len();
let value = line.get(start..)?.trim();
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
- Prefer raw string literals for regexes and backslash-heavy path examples to avoid malformed escapes.
- Before documenting or logging a recommended config value, trace how the consumer uses it. For example, `environmentDirectories` contains directories that hold environments, not environment folders themselves.

## Subprocess Ownership

Keep Windows probe children suspended until job assignment succeeds, and fail closed if the primary thread cannot be identified. On stable Rust, a per-process `PssCaptureSnapshot(PSS_CAPTURE_THREADS)` can supply thread metadata without a system-wide Toolhelp scan or address-space clone; benchmark the launch boundary against the exact base on the same host rather than assuming enumeration is cheap. `CommandExt::creation_flags` replaces existing flags, so any runner-controlled flag contract must be explicit and tested. On Unix, signal the owned process group before reaping its leader (`waitid` with `WNOWAIT` preserves the PID until then), never after the numeric group ID could be reused.

On Darwin, signalling a zombie-only group can return `EPERM`; never suppress that error without proving there are no live members. `proc_pidinfo(PROC_PIDTBSDINFO)` requires a nonzero argument to include zombies, and any group inspection must remain bounded and revalidate member birth identities rather than trusting PIDs alone.

For descendant-lifetime tests, use a readiness handshake and an OS-owned resource such as a file lock that is released on exit. Do not equate Unix PID disappearance with termination: grandchildren can be dead but still awaiting reaping by their parent or the OS reaper.

## Tests Must Prove the Change

Tests should demonstrate the behavior or performance invariant, not merely execute new lines.

For optimizations, instrument the dependency boundary and assert the operation count:

```rust
let reads = AtomicUsize::new(0);
parse_with_reader(path, |_| {
    reads.fetch_add(1, Ordering::Relaxed);
    Some(history.clone())
});
assert_eq!(reads.load(Ordering::Relaxed), 1);
```

For parser helpers, include malformed input, non-ASCII surrounding data, and case variations. For diagnostics, test pattern classification and expansion filtering separately. Keep temp paths unique with `tempfile` or process/counter-based names.

Before every Rust commit, run targeted tests and invoke the `rust-precommit` skill. Keep that skill as the single source of truth for required format and Clippy commands.

## Learnings

Do not execute freshly written scripts as concurrent Unix subprocess fixtures: spawning can fail with `ETXTBSY` (Text file busy). Prefer an existing interpreter such as `/bin/sh -c` with an inline script, or the existing test executable. Assert the typed runner outcome before checking an optional parsed result, so a spawn failure cannot masquerade as a successful negative parsing or timeout test.

For real-pipe EOF/EPIPE tests, create the pipe inside an isolated test subprocess when other test threads spawn children. Unix `CLOEXEC` closes descriptors at exec, not fork: a concurrent child can temporarily retain a reader, allowing the only write to succeed before the final reader disappears. A readiness handshake alone does not prevent this race. Keep the operation's measured deadline separate from setup, and make an outer fixture deadline cover readiness, waits both before and after forced termination, reader joins, and fallback `Drop` cleanup.

Use a per-worktree Cargo target directory when validating stacked changes so native fixtures cannot execute another worktree's stale binary. On WSL, run timing-sensitive Linux binaries from the native Linux filesystem rather than a Windows mount, where page faults can stall in filesystem RPC. When launching instrumented PET with `env_clear()`, retain `LLVM_PROFILE_FILE` exactly so child coverage reaches the collector instead of an uncollected default profile.
