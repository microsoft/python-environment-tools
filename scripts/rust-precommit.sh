#!/usr/bin/env bash
# Rust pre-commit checks for PET workspace.
# Runs `cargo fmt --all` and `cargo clippy --all -- -D warnings`.
# Exits non-zero on any failure so callers can gate commits.

set -euo pipefail

WORKSPACE="${1:-.}"
cd "$WORKSPACE"

echo "=== RUST PRE-COMMIT CHECKS ==="

if ! command -v cargo &> /dev/null; then
    if [ -f "$HOME/.cargo/env" ]; then
        # shellcheck disable=SC1091
        source "$HOME/.cargo/env"
    else
        echo "FAIL: cargo not found in PATH" >&2
        exit 1
    fi
fi

echo ""
echo "--- Step 1: cargo fmt --all ---"
if ! cargo fmt --all; then
    echo "FAIL: cargo fmt failed." >&2
    exit 1
fi
echo "PASS: Formatting complete."

echo ""
echo "--- Step 2: cargo clippy --all -- -D warnings ---"
if ! cargo clippy --all -- -D warnings; then
    echo "FAIL: clippy reported errors. Fix them, then re-run." >&2
    exit 1
fi
echo "PASS: Clippy clean."

echo ""
echo "=== ALL PRE-COMMIT CHECKS PASSED ==="
echo "Safe to commit."
