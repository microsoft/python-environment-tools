# Quality snapshots

PET uses pull-request snapshots to prevent performance and coverage drift. Each pull request is compared with artifacts produced for the exact pull-request base commit, not the latest moving `main` tip.

## Performance gate

The performance workflow runs 10 paired cache-cold/cache-warm JSON-RPC iterations on Linux,
Windows, and macOS, plus 10 untimed cache-cold diagnostic iterations. A comparison is valid only
when every required distribution has at least five samples, inventories match within the same
inventory schema, and both benchmark execution and JSON extraction succeed.

Schema v3 separates client-observed operation latency from server attribution:

- `refresh_round_trip` measures immediately before request serialization/write through reading
  the matching response. It includes parsing, glob expansion, queueing, discovery, pre-reply
  synchronization, and transport.
- `request_time_to_first_env` uses that operation boundary and records the first environment
  notification read before the matching response. The observation resets before every refresh.
- `startup_time_to_first_env` remains process-spawn-relative and never resets.
- `discovery_duration` retains the server's discovery-only `RefreshResult.duration` for attribution.
- cold distributions use the same boundaries; phase, locator, and timeout data remain diagnostics.

The benchmark clears local notification state before starting the operation clock, keeps one
buffered stdout reader for the process lifetime, and continuously drains a bounded stderr tail.
Environment notifications have no request identifier, so request-relative TTFE is not claimed as
concurrency-safe attribution and post-response notifications cannot be assigned to a request. Each
measured refresh uses a fresh process to avoid that ambiguity. It must produce every required
sample; missing data is invalid rather than silently skipped.

A metric blocks only when it exceeds both its absolute and relative budget. Schema v3 keeps the
existing discovery/startup-relative gates and applies the corresponding unchanged tolerances to
client round-trip/request-relative metrics once both exact-base snapshots are v3:

| Metric family | Linux | Windows | macOS |
| --- | ---: | ---: | ---: |
| Server startup P50 | 5 ms / 100% | 10 ms / 50% | 100 ms / 50% |
| Server startup P95 | 50 ms / 200% | 50 ms / 100% | 750 ms / 100% |
| Discovery duration and refresh round-trip P50 | 25 ms / 30% | 150 ms / 50% | 100 ms / 50% |
| Discovery duration and refresh round-trip P95 | 50 ms / 50% | 250 ms / 100% | 300 ms / 100% |
| Startup/request-to-first environment P50 | 20 ms / 100% | 25 ms / 50% | 150 ms / 50% |
| Startup/request-to-first environment P95 | 25 ms / 100% | 100 ms / 100% | 250 ms / 100% |
| Cold discovery duration and refresh round-trip P50 | 100 ms / 50% | 150 ms / 50% | 250 ms / 50% |

Each cell is `absolute / relative`. No threshold was widened for schema v3. These client thresholds
are conservative initial gates, not a claimed cross-platform calibration: release acceptance still
requires comparable repeated schema-v3 baselines on all three hosted-runner platforms. Those runs
must be reviewed before changing any threshold.

### Exact-base schema transition

Schema v2's `full_refresh` is discovery-only and `time_to_first_env` is startup-relative. During an
exact-base v2 to current v3 comparison, the comparator maps only those semantically identical values
to `discovery_duration` and `startup_time_to_first_env`, preserving every existing gate. All new v3
distributions are still mandatory, but client metrics are explicitly reported as transition
diagnostics because a v2 artifact has no comparable samples. They are never relabeled or compared
to discovery duration. As soon as the exact base publishes v3, round-trip and request-relative gates
activate automatically. A v2 current snapshot against a v3 base, unknown versions, missing metrics,
and insufficient samples fail closed.

Schema v2's earlier v1 transition still checks cold discovery P50 against explicit ceilings of 500ms
on Linux, 750ms on Windows, and 1,000ms on macOS. Inventory schema v2 likewise permits count changes
only during its one-time v1-to-v2 transition; equal inventory schemas require exact counts.

The macOS startup P95 calibration remains tracked by #507. Warm discovery/startup-to-first P95 was
recalibrated in #511, Windows warm discovery P50 in #513, and cold discovery P50 in #509. These
historical calibrations apply only to their unchanged semantic metrics, not as fabricated evidence
for the new client clocks.

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
