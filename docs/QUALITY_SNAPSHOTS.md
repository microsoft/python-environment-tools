# Quality snapshots

PET uses pull-request snapshots to prevent performance and coverage drift. Each pull request is compared with artifacts produced for the exact pull-request base commit, not the latest moving `main` tip.

## Performance gate

The performance workflow runs 10 end-to-end JSON-RPC iterations on Linux, Windows, and macOS. A comparison is valid only when:

- current and baseline metrics contain at least five samples for every required distribution;
- environment and manager counts match exactly; and
- the benchmark command and JSON extraction both succeed.

A metric blocks when it exceeds both its absolute and relative budget:

| Metric | Linux | Windows | macOS |
| --- | ---: | ---: | ---: |
| Server startup P50 | 5 ms / 100% | 10 ms / 50% | 100 ms / 50% |
| Server startup P95 | 50 ms / 200% | 50 ms / 100% | 10,000 ms / 100% |
| Full refresh P50 | 25 ms / 30% | 50 ms / 30% | 100 ms / 50% |
| Full refresh P95 | 1,000 ms / 100% | 5,000 ms / 100% | 5,000 ms / 25% |
| Time to first environment P50 | 20 ms / 100% | 25 ms / 50% | 150 ms / 50% |
| Time to first environment P95 | 250 ms / 100% | 500 ms / 100% | 10,000 ms / 100% |

Each cell is `absolute / relative`. The budgets reflect observed GitHub-hosted runner variance from 11 consecutive main-branch baselines. Tighten them when a noisy path is fixed rather than normalizing a known regression into the baseline.

The dual budget avoids failing on tiny percentage changes while still blocking material latency regressions. Tail metrics remain mandatory; a healthy median does not excuse a degraded P95.

## Coverage gate

Linux and Windows line and function coverage are compared with the exact base commit. A decrease greater than 0.01 percentage points blocks the pull request. Coverage artifacts and comments remain available for inspection even when the comparison fails.

## Running locally

```powershell
python -m unittest discover -s scripts/tests -p 'test_*.py' -v
python scripts/quality_snapshot.py performance --current metrics.json --baseline baseline.json --platform Windows --report report.md
python scripts/quality_snapshot.py coverage --current lcov.info --baseline baseline.info --platform Windows --report report.md
```

Run the E2E benchmark with:

```powershell
cargo test --release --features ci-perf --test e2e_performance test_performance_summary -- --nocapture
```

The E2E client keeps one buffered stdout reader for the process lifetime and continuously drains a bounded stderr tail so protocol read-ahead and pipe backpressure cannot distort measurements.

## Known investigations

The persistent macOS cold-refresh tail is tracked by issue #504. Existing tail latency is represented in the baseline, but any further regression is still gated.
