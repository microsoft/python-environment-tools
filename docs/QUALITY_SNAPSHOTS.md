# Quality snapshots

PET uses pull-request snapshots to prevent performance and coverage drift. Each pull request is compared with artifacts produced for the exact pull-request base commit, not the latest moving `main` tip.

## Performance gate

The performance workflow runs 10 paired cache-cold/cache-warm JSON-RPC iterations on Linux, Windows, and macOS, plus 10 untimed cache-cold diagnostic iterations. A comparison is valid only when:

- current and baseline metrics contain at least five samples for every required distribution;
- environment and manager counts match exactly within the same inventory schema; and
- the benchmark command and JSON extraction both succeed.

A metric blocks when it exceeds both its absolute and relative budget:

| Metric | Linux | Windows | macOS |
| --- | ---: | ---: | ---: |
| Server startup P50 | 5 ms / 100% | 10 ms / 50% | 100 ms / 50% |
| Server startup P95 | 50 ms / 200% | 50 ms / 100% | 750 ms / 100% |
| Full refresh P50 | 25 ms / 30% | 150 ms / 50% | 100 ms / 50% |
| Full refresh P95 | 50 ms / 50% | 250 ms / 100% | 300 ms / 100% |
| Time to first environment P50 | 20 ms / 100% | 25 ms / 50% | 150 ms / 50% |
| Time to first environment P95 | 25 ms / 100% | 100 ms / 100% | 250 ms / 100% |
| Cold refresh P50 | 100 ms / 50% | 150 ms / 50% | 250 ms / 50% |

Each cell is `absolute / relative`. The Linux/macOS warm P50 and all server-startup budgets reflect observed GitHub-hosted runner variance from 11 consecutive main-branch baselines. Tighten them when a noisy path is fixed rather than normalizing a known regression into the baseline.

The macOS server-startup P95 budget recalibration is tracked by issue #507 and follows PR #506's fix for issue #504. It uses three unchanged-content pull-request runs and the exact merged baseline at `f0c62d9`; the resulting absolute headroom is four to six times the observed post-fix run-to-run range.

The warm refresh and warm time-to-first P95 budgets were recalibrated in issue #511 after PR #510 separated cold and warm samples. Three unchanged-code PR runs plus the exact schema-v2 baseline at `ad7ca14` retain at least 2.5 times the observed absolute run-to-run range.

The Windows warm full-refresh P50 budget was recalibrated in issue #513 from five unchanged-code pull-request measurements plus the exact schema-v2 baseline at `ad7ca14` (six measurements total). It retains nearly twice the observed absolute range while blocking a sustained median above 255ms against that baseline.

Schema v2 records `full_refresh` and `time_to_first_env` from the warm member of each pair and adds cold refresh/time-to-first distributions. During its one-time rollout, comparisons against a schema-v1 base checked cold P50 against explicit absolute ceilings of 500ms on Linux, 750ms on Windows, and 1,000ms on macOS. Schema-v2-to-v2 comparisons use the table's dual budgets.

Inventory schema v2 treats Windows Conda installation paths that differ only by on-disk casing as one logical workload entry. During the one-time v1-to-v2 transition, the report explicitly identifies the schema change and permits the expected count mismatch. Once the v2 baseline is published, exact environment and manager count matching resumes automatically.

The cold P50 budgets were calibrated in issue #509 using two unchanged-head all-platform runs and the final pull-request validation.

The dual budget avoids failing on tiny percentage changes while still blocking material latency regressions. Warm tail metrics remain mandatory; cold P95 remains diagnostic because a single host event can dominate it, while cold P50 blocks delays that affect the independent cold iterations consistently.

## Coverage gate

Linux and Windows line and function coverage are compared with the exact base commit. A decrease greater than 0.01 percentage points blocks the pull request. Coverage artifacts and comments remain available for inspection even when the comparison fails.

## Running locally

The comparator requires Python 3.10 or newer.

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
Each cold/warm pair uses a unique cache directory and a fresh PET process for each member. Phase and locator telemetry is collected across separate, independently cold untimed refreshes so diagnostic processing cannot backpressure the timed JSON-RPC refreshes.

## Known investigations

The macOS cold-refresh tail fixed by PR #506 (tracked in issue #504) remains guarded by phase and locator distributions plus privacy-safe interpreter timeout counts.
The cold/warm sampling design is tracked by issue #509.
