# Quality snapshots

PET uses pull-request snapshots to prevent performance and coverage drift. Each pull request is compared with artifacts produced for the exact pull-request base commit, not the latest moving `main` tip.

## Performance gate

The performance workflow runs 10 paired cache-cold/cache-warm JSON-RPC iterations on Linux,
Windows, and macOS, plus 10 untimed cache-cold diagnostic iterations. A comparison is valid only
when every required distribution has at least five samples, inventories match within the same
inventory schema, and both benchmark execution and JSON extraction succeed.

Within one benchmark invocation, every measured cold/warm refresh and untimed diagnostic refresh
must report the same environment and manager identities, not merely the same counts. Environments
are compared by normalized executable/prefix, kind and version; managers by normalized executable,
tool and version. Notification order is ignored, but duplicate multiplicity is preserved. Entries
without an environment executable/prefix or a manager executable/tool are invalid. PET path
normalization plus Windows ASCII case folding preserves symlink/junction identity and Unix case
distinctions. Normalization and comparison run after the refresh timer stops; mismatch diagnostics
report only category, iteration and counts, never identity paths. Serialized inventories remain
counts only: cross-run PR/base comparison and the existing metric/inventory schema versions are
unchanged.

Schema v3 separates client-observed operation latency from server attribution:

- `refresh_round_trip` measures immediately before request serialization/write through reading
  the matching response. It includes parsing, glob expansion, queueing, discovery, pre-reply
  synchronization, and transport.
- `request_time_to_first_env` uses that operation boundary and records the first environment
  notification read before the matching response. The observation resets before every refresh.
- `startup_time_to_first_env` remains process-spawn-relative and never resets.
- `discovery_duration` retains the server's discovery-only `RefreshResult.duration` for attribution.
- cold distributions use the same boundaries; phase, locator, and timeout data remain diagnostics.

The benchmark clears collected inventories and progress before starting the operation clock,
keeps one buffered stdout reader for the process lifetime, and continuously drains a bounded stderr
tail. The request-relative observation closes after a refresh response or error; notifications read
by later non-refresh requests cannot fill a missing TTFE sample for the completed refresh.
Environment notifications have no request identifier, so request-relative TTFE is not claimed as
concurrency-safe attribution and post-response notifications cannot be assigned to a request. Each
measured refresh uses a fresh process to avoid that ambiguity. It must produce every required
sample; missing data is invalid rather than silently skipped. The standalone full-refresh benchmark
also rejects missing request-to-first samples.

A metric blocks only when it exceeds both its absolute and relative budget. Schema v3
preserves all existing discovery/startup-relative gates:

| Existing metric | Linux | Windows | macOS |
| --- | ---: | ---: | ---: |
| Server startup P50 | 5 ms / 100% | 10 ms / 50% | 100 ms / 50% |
| Server startup P95 | 50 ms / 200% | 50 ms / 100% | 750 ms / 100% |
| Discovery duration P50 | 25 ms / 30% | 150 ms / 50% | 100 ms / 50% |
| Discovery duration P95 | 50 ms / 50% | 250 ms / 100% | 300 ms / 100% |
| Startup-to-first environment P50 | 20 ms / 100% | 25 ms / 50% | 150 ms / 50% |
| Startup-to-first environment P95 | 25 ms / 100% | 100 ms / 100% | 250 ms / 100% |
| Cold discovery duration P50 | 100 ms / 50% | 150 ms / 50% | 250 ms / 50% |

Client gates use their own calibration, active only when both exact-base snapshots are v3:

| New client metric | Linux | Windows | macOS |
| --- | ---: | ---: | ---: |
| Refresh round-trip P50 | 25 ms / 30% | 150 ms / 50% | 250 ms / 50% |
| Refresh round-trip P95 | 50 ms / 50% | 250 ms / 100% | 300 ms / 100% |
| Request-to-first environment P50 | 20 ms / 100% | 25 ms / 50% | 50 ms / 50% |
| Request-to-first environment P95 | 25 ms / 100% | 100 ms / 100% | 100 ms / 100% |
| Cold refresh round-trip P50 | 100 ms / 50% | 150 ms / 50% | 600 ms / 50% |

Each cell is `absolute / relative`. Initial client calibration in #531 uses four runs of
unchanged benchmark source at `9c1b003`: [baseline 1](https://github.com/microsoft/python-environment-tools/actions/runs/35669490808),
[baseline 2](https://github.com/microsoft/python-environment-tools/actions/runs/35669544096),
[baseline 3](https://github.com/microsoft/python-environment-tools/actions/runs/35669544153), and
[PR measurement](https://github.com/microsoft/python-environment-tools/actions/runs/35669495406).
Each platform has 40 cold/warm pairs; inventories were stable at 5/8/10 environments on
Linux/Windows/macOS respectively, with one manager. The PR Windows artifact upload hit HTTP 403;
its successful benchmark/comparison JSON was recovered from the job log instead of discarded.

Observed run-to-run ranges (maximum minus minimum statistic, not individual-sample spread):

| Client statistic range | Linux | Windows | macOS |
| --- | ---: | ---: | ---: |
| Round-trip P50 / P95 | 11 / 10 ms | 56 / 63 ms | 105 / 105 ms |
| Request-to-first P50 / P95 | 3 / 4 ms | 7 / 17 ms | 19 / 41 ms |
| Cold round-trip P50 | 41 ms | 59 ms | 256 ms |

Client absolute budgets retain at least twice those observed ranges, rounded up; existing
larger Linux/Windows tolerances were retained. Relative tolerances remain unchanged. macOS
request-relative TTFE tolerances are tighter than the unrelated startup-relative ones. Directly
reusing discovery tolerances for macOS client P50/cold P50 falsely rejected unchanged source.
No existing discovery, startup, or coverage gate is relaxed: they can still block independently.
These are initial hosted-runner noise budgets, not user latency SLOs. Cold P95 stays diagnostic
because individual host events dominate it; recalibrate only with new comparable evidence.

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
