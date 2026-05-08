---
name: "rust-precommit"
description: "Run the mandatory PET Rust pre-commit checks (cargo fmt + cargo clippy -D warnings) before every commit. Invoke whenever you are about to commit Rust changes, after applying review feedback, or whenever you need to verify formatting and lint cleanliness."
---

# Rust Pre-Commit Checks

The PET maintainer workflow requires `cargo fmt --all` and `cargo clippy --all -- -D warnings` to pass before every commit. This skill wraps both into a single script invocation.

## When to use

- Before staging any Rust changes for commit.
- After fixing review comments on a PR, before pushing.
- Whenever the agent is asked "run pre-commit checks" or "validate the Rust code".

## How to run

From the workspace root:

### Windows (pwsh)

```powershell
./scripts/rust-precommit.ps1
```

### macOS / Linux

```bash
./scripts/rust-precommit.sh
```

Both scripts:

1. Run `cargo fmt --all` (auto-formats; fails only on unrecoverable errors).
2. Run `cargo clippy --all -- -D warnings` (treats warnings as errors).
3. Exit non-zero on the first failure so callers can gate commits.

## Handling failures

- **fmt failure:** rare; usually a syntax error preventing parsing. Fix the underlying compile error first.
- **clippy failure:** read the diagnostic, fix the code. Do **not** add `#[allow(...)]` to suppress unless absolutely justified.
- After any fix, re-run the script until it reports `ALL PRE-COMMIT CHECKS PASSED`.

## Notes

- The script must be run from the repository root (or pass the workspace path as the first arg to the `.sh` variant / `-WorkspacePath` to the `.ps1` variant).
- Keep the script in sync with `.github/copilot-instructions.md` "Required Before Committing" section.
