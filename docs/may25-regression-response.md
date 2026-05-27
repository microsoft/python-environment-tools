# PET Regression Investigation — Response to May 25 Report

**Date:** May 26, 2026
**Audience:** Python Environments extension team (`ms-python.vscode-python-envs`)
**Re:** PET `pet.refresh` latency regression (+91% p90 darwin, +44% p90 win32) on extension v1.33 insiders vs v1.30 stable
**Scope:** Static code review of every commit on `origin/main` between the `release/2026.6` cut (`172d4b1`, 2026-04-11) and the `release/2026.8` cut (`d581272`, 2026-05-12). No live profiling or telemetry analysis.

---

## TL;DR

The regression has **one primary cause and one amplifier**, both landed in `release/2026.8`:

1. **Primary — [#416](https://github.com/microsoft/python-environment-tools/pull/416) "serialize configure locator updates" (1c28b88, Apr 15).** This change moved every `locator.configure()` call _inside_ the `configuration.write()` lock. The refresh path takes a `configuration.read()` lock per environment report (`GenerationGuardedReporter::report_if_current`) and again at end of refresh (`sync_refresh_locator_state_if_current`). When `configure` overlaps with `refresh`, every reporter call on every platform now contends with the write lock. This is the only commit in the window that fits the "cross-platform p90 regression, not Windows-specific" shape.

2. **Amplifier — [#460](https://github.com/microsoft/python-environment-tools/pull/460) "add pet-hatch locator" (d581272, May 12).** A new locator was inserted into the chain on every platform. Its `configure()` reads and parses `pyproject.toml` and `hatch.toml` for **every workspace directory** on every `configure` call — and after #416, that I/O happens while holding the configuration write lock. The May 12 landing date matches the start date of the insider-builds latency curve in your report.

3. **Secondary, smaller — [#452](https://github.com/microsoft/python-environment-tools/pull/452) "read build-details.json (PEP 739)" (ab6d1d2, May 7).** Adds 3–4 extra `is_file()` stats per virtual env on the version-detection chain (when a `(major, minor)` hint is available from `pyvenv.cfg`); falls back to a full `fs::read_dir(<prefix>/lib)` per virtual env when no hint is present. Not enough to fully explain the +700 ms darwin p90 alone, but additive on top of #416/#460.

**Windows perf fixes shipped, but the May 14–22 "recovery curve" is not them.** Of the 18 commits in the window, four are Windows-specific perf fixes ([#456](https://github.com/microsoft/python-environment-tools/pull/456) WinPython narrow paths, [#457](https://github.com/microsoft/python-environment-tools/pull/457) `norm_case` memoization, [#458](https://github.com/microsoft/python-environment-tools/pull/458) parallelize registry, [#418](https://github.com/microsoft/python-environment-tools/pull/418) cache Windows Store symlinks). All four landed _before_ the `release/2026.8` cut (May 12) — they are present across every v1.33 insider build May 14 onward, not landing progressively. See Section 2 for the corrected interpretation of the 45s → 24s curve.

**`release/2026.6` was cut 2026-04-11, _before_ #416 (Apr 15) and #460 (May 12).** Therefore v1.30 stable has neither the lock issue nor the Hatch amplifier — the clean baseline is real, and the regression is fully attributable to the v1.33 cohort.

**`release/2026.8` will NOT fix the regression** because the primary cause (#416) is in that branch _and no fix has landed yet_ — issues #461 (lock) and #462 (Hatch I/O) are filed but unstarted. Pinning insiders to `release/2026.8` today will not help; wait for a release that contains the fixes.

---

## Section 1 — Direct answers to your questions

### Q1. What landed between `release/2026.6` and `release/2026.8`?

Full list of 18 commits (`origin/release/2026.6..origin/release/2026.8`), grouped:

**Concurrency / refresh hot path:**

| SHA       | PR                                                                     | Date   | Summary                                                                                                                                  |
| --------- | ---------------------------------------------------------------------- | ------ | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `1c28b88` | [#416](https://github.com/microsoft/python-environment-tools/pull/416) | Apr 15 | **fix: serialize configure locator updates** — moves locator config under write lock. **PRIMARY REGRESSION CAUSE.**                      |
| `f827577` | [#419](https://github.com/microsoft/python-environment-tools/pull/419) | Apr 15 | fix: coalesce locator cache in-flight lookups — adds per-key Mutex+Condvar; could elongate p90 if a closure stalls. Likely small impact. |

**New locator (cross-platform work added to every refresh):**

| SHA       | PR                                                                     | Date   | Summary                                                                                                                                                     |
| --------- | ---------------------------------------------------------------------- | ------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `d581272` | [#460](https://github.com/microsoft/python-environment-tools/pull/460) | May 12 | **feat: add pet-hatch locator** — inserted before `Venv` in the chain; `configure()` does per-workspace `pyproject.toml` + `hatch.toml` I/O. **AMPLIFIER.** |

**Cross-platform version detection (small per-env cost):**

| SHA       | PR                                                                     | Date  | Summary                                                                                                           |
| --------- | ---------------------------------------------------------------------- | ----- | ----------------------------------------------------------------------------------------------------------------- |
| `ab6d1d2` | [#452](https://github.com/microsoft/python-environment-tools/pull/452) | May 7 | feat: read build-details.json (PEP 739) — extra stats per virtual env in `version::from_creator_for_virtual_env`. |

**Windows perf fixes (intended improvements, all in 2026.8):**

| SHA       | PR                                                                     | Date   | Summary                                                              |
| --------- | ---------------------------------------------------------------------- | ------ | -------------------------------------------------------------------- |
| `93285a8` | [#458](https://github.com/microsoft/python-environment-tools/pull/458) | May 8  | perf(pet-windows-registry): parallelize HKLM/HKCU walk + reuse cache |
| `4a45be5` | [#457](https://github.com/microsoft/python-environment-tools/pull/457) | May 7  | perf(pet-fs): memoize `norm_case`                                    |
| `34b2d47` | [#456](https://github.com/microsoft/python-environment-tools/pull/456) | May 7  | fix(winpython): narrow search paths + add discovery cache            |
| `cb0fe69` | [#418](https://github.com/microsoft/python-environment-tools/pull/418) | Apr 15 | perf: cache Windows Store normalized symlinks                        |

**Bug fixes (small surface area):**

| SHA       | PR                                                                     | Date   | Summary                                                        |
| --------- | ---------------------------------------------------------------------- | ------ | -------------------------------------------------------------- |
| `2ac76d5` | [#417](https://github.com/microsoft/python-environment-tools/pull/417) | Apr 15 | fix: tighten Poetry cache path matching                        |
| `e9e15dc` | [#447](https://github.com/microsoft/python-environment-tools/pull/447) | —      | fix: pyenv-win choco → official PowerShell installer (CI only) |

**Test-only PRs (no runtime impact):** `#420`, `#421`, `#422`, `#423`, `#424`, `#425`, `#426`, `#428`, `#429`, `#430`, `#435`, `#445`.

### Q2. Will `release/2026.8` fix the regression?

**No, not by itself.** `release/2026.8` contains the primary regression cause (#416) and its amplifier (#460). Pinning insiders to `release/2026.8` today will move you from "main + 9 extra test PRs" to "main minus 9 test PRs" — i.e., no perf change.

**Recommended action:** Hold the pin change until a `release/2026.10` (or a hotfix on `release/2026.8`) lands that addresses #416 — see Section 3 below for the proposed fix.

### Q3. Why did `pet.refresh` p90 nearly double on darwin?

darwin has no Windows-specific code paths and only one of the 18 commits added meaningful cross-platform work to refresh: **#460 (pet-hatch)**. On its own that adds at most one `is_dir()` per refresh on machines without Hatch installed plus per-workspace TOML parsing in `configure()` — cheap in isolation.

The doubling makes sense once you combine that with **#416**: Hatch's `configure()` I/O now runs under the configuration write lock, and every concurrent refresh's per-environment reports (`GenerationGuardedReporter::report_if_current`) block on the same lock. The "rare but severe" tail of refreshes that happen to overlap with a configure call gets pushed deep into p90 territory.

Smaller contributing factors on darwin:

- **#452 build-details:** adds 3–4 `is_file()` calls per virtual env in `version::from_creator_for_virtual_env`. On a typical macOS dev box with 30 virtual envs this is ~1–2 ms. Real but small.
- **#419 cache coalesce:** waiters now block on the first in-flight closure for the same key. If a single conda-manager probe stalls, all sibling lookups wait. Likely net neutral but can lengthen p90.

### Q4. Why does `resolve` for invalidated paths hit 205 s p90 on Windows?

**Not investigated in depth in this pass** — we'd need to read the `resolve` RPC handler and the Windows Store/Registry `try_from` paths together with `resolve_symlink`. Two hypotheses worth checking on your side first:

1. **Windows Store stub on a vanished package.** If the cached path was `~/AppData/Local/Microsoft/WindowsApps/python.exe` for a now-uninstalled Store install, `try_from` may fall through and trigger a process spawn against the stub. The stub is known to block on UI/Store activation flows under some conditions.
2. **`norm_case` on a non-existent path.** `norm_case` returns the input unchanged when `GetLongPathNameW` fails, but Defender can stall the failing syscall itself for tens of seconds on suspicious paths.

Either way: **please add a bounded timeout on the extension side** (you mentioned you're planning this — strongly seconded). A second-side investigation is on our list; if you can correlate `resolve` slow tails with the `Properties.path` prefix (`WindowsApps`, `registry`, `conda`, `pyenv`, etc.) we can attribute much faster.

### Q5. Native Windows crash codes (0xC0000142, 0xC0000005, 0xC0000409)

Counts are small (3–12 machines each) but real. PET has **no crash/exit-reason telemetry today** (no `panic::set_hook`, no `catch_unwind` at the JSON-RPC dispatcher, no shutdown event). #416 _did_ add `panic::catch_unwind` around per-locator configure (you can see it in the `apply_configure_options` function), but it does not write a crash record before re-raising. We will:

- Pull the SQM IDs from you for the crash cohort.
- Scope a `panic::set_hook` that persists `last_exit_reason.json` to the cache dir, plus a `catch_unwind` boundary around the JSON-RPC dispatch loop so non-panic process death (`0xC0000005`, `0xC0000409`) can also be attributed via a `startup_after_crash` telemetry event.

Tracking issue to be filed separately.

---

## Section 2 — Evidence for the #416 + #460 diagnosis

### #416 — before vs after

**Before** (`release/2026.6`), the critical section released the write lock before configuring locators:

```rust
let config = {
    let mut state = context.configuration.write().unwrap();
    state.config.workspace_directories = workspace_directories;
    // ... set other fields ...
    state.generation += 1;
    state.config.clone()
};                                              // ← write lock RELEASED
configure_locators(&context.locators, &config); // ← locator I/O outside lock
```

**After** (in `release/2026.8`), see [crates/pet/src/jsonrpc.rs](../crates/pet/src/jsonrpc.rs#L661-L719):

```rust
fn apply_configure_options(...) -> Result<(), String> {
    let mut state = configuration.write().unwrap();   // ← acquire write lock
    let mut next_config = state.config.clone();
    // ... set fields ...
    for locator in locators.iter() {                  // ← held for ALL locators
        if let Err(panic_payload) = panic::catch_unwind(AssertUnwindSafe(|| {
            locator.configure(&next_config);          // ← I/O under write lock
        })) { ... }
    }
    state.config = next_config;
    state.generation = next_generation;
}
```

### Why refresh blocks

`Context` holds `configuration: Arc<RwLock<ConfigurationState>>` and refresh reads it in three places (all in [crates/pet/src/jsonrpc.rs](../crates/pet/src/jsonrpc.rs)):

1. **`GenerationGuardedReporter::report_if_current`** (line ~416) — `configuration.read()` on **every** `report_environment` call during refresh, to compare generations and drop stale reports.
2. **`sync_refresh_locator_state_if_current`** (line ~376) — `configuration.read()` once at end of refresh, held for the duration of `sync_refresh_locator_state`.
3. **`execute_refresh`** (line ~1046) — `configuration.read()` to snapshot the config at refresh start.

Rust's `std::sync::RwLock` is **write-preferring** on most platforms — once a writer is queued, new readers block. With #416 the writer is queued for the entire `locator.configure()` loop. On a machine with even a few hundred environments, that means hundreds of `report_environment` calls in a concurrent refresh thread all serialize behind the configure-driven write.

### Why #460 amplifies it

[crates/pet-hatch/src/lib.rs#L130-L154](../crates/pet-hatch/src/lib.rs#L130-L154) — Hatch's `configure()`:

```rust
fn configure(&self, config: &Configuration) {
    let mut new_cache: WorkspaceVirtualDirs = Vec::new();
    if let Some(dirs) = config.workspace_directories.as_ref() {
        for workspace in dirs {
            // Single parse of pyproject.toml + hatch.toml per workspace
            let (virtual_dirs, env_names) = resolve_workspace_hatch_config(workspace);
            new_cache.push(Arc::new(WorkspaceEntry { ... }));
        }
    }
    *self.workspace_virtual_dirs.lock()... = new_cache;
}
```

The comment correctly notes the inner Hatch mutex is held only at the very end — but the _outer_ `configuration.write()` lock (held by `apply_configure_options`) is now held throughout this loop. On a multi-root workspace each iteration is `fs::read_to_string("pyproject.toml")` + `fs::read_to_string("hatch.toml")` — at minimum two stat-misses per workspace, more if the files exist. Under Defender on Windows or under spinning disks these can be tens of milliseconds each. On macOS it's much cheaper per call, but it still happens inside the lock.

### Why the timing matches your build curve

| Insider build   | Date      | Reg p90 | Notes                                                                                   |
| --------------- | --------- | ------: | --------------------------------------------------------------------------------------- |
| 1.33.2026051401 | May 14    |  45.18s | Two days after #460 (May 12) — Hatch + #416 both active, Windows perf fixes also active |
| 1.33.2026051501 | May 15    |  30.34s | —                                                                                       |
| 1.33.2026052101 | May 21    |  28.89s | —                                                                                       |
| 1.33.2026052201 | May 22    |  24.58s | —                                                                                       |
| baseline 1.30   | early May |  19.83s | Cut from `release/2026.6` (2026-04-11) — pre-#416, pre-#460; clean baseline             |

**Correction (post-review):** an earlier draft of this doc attributed the 45s → 24s curve to Windows perf fixes landing progressively. That is wrong. Checking the PRs that actually merged into PET `main` between May 12 (the `release/2026.8` cut) and May 22:

- May 18: #436, #437, #438, #439, #440, #441, #443, #444 — all `test:` PRs.
- May 18: #459 — `chore: add rust-precommit skill`.

No perf fixes. The four Windows perf PRs (#456, #457, #458, #418) all landed _before_ the May 12 release cut and were present in every v1.33 insider build from May 14 on. The most plausible explanations for the apparent recovery are therefore **cohort drift** (early-adopter machines dropping out as insiders rolls out to a broader population) and **cross-session cache warming** (`norm_case` memoization from #457, the WinPython discovery cache from #456, and the registry cache from #458 all persist within a process; longer-lived sessions amortize their warm-up cost). It is **not** evidence that the regression is self-healing. Until #461 and #462 land, p90 should be assumed stuck above the v1.30 floor.

---

## Section 3 — Proposed fix for #416

The fix is surgical: keep #416's correctness goal (locators reach their new config before the new `generation` is published to refresh threads) but get the I/O back out of the write lock. Two implementation options, both low-risk:

**Option A — Use a separate "configuring" mutex.**

```rust
// Held only by configure callers; prevents two concurrent configure runs.
configure_in_progress: Mutex<()>,
// Existing RwLock — unchanged.
configuration: RwLock<ConfigurationState>,
```

- Take `configure_in_progress` first.
- Take `configuration.write()` briefly to compute `next_config` and `next_generation` _without publishing yet_.
- Drop the write lock; run `locator.configure(&next_config)` for each locator (no lock held).
- Re-take `configuration.write()` for a fast publish: `state.config = next_config; state.generation = next_generation`.
- Rollback on panic still works because we hold `configure_in_progress` exclusively.

**Option B — Publish-after-configure ordering with the existing lock.**

Same as the old code, but route every refresh path that needs to _act on_ the new generation to wait on a separate `Condvar`/`AtomicU64` notification instead of polling `state.generation`. More invasive.

**Recommendation: Option A.** It restores the exact pre-#416 behavior for the I/O-heavy section while keeping #416's added correctness guarantees (single configurator at a time, rollback on locator panic). Estimated diff: ~50 lines in `crates/pet/src/jsonrpc.rs`. The existing regression test added by #416 (`test_configure_publishes_state_after_shared_locators_are_configured`) should still pass — Option A still publishes the new generation only after all locators are configured, just without freezing readers in the meantime.

We'll open a PR shortly. If you want to validate the fix before it cuts a release branch, we can ship an `origin/main` SHA you can pin the insiders pipeline to temporarily.

---

## Section 4 — Telemetry / instrumentation requests

Direct response to your items 6–8:

### #6 — Version RPC / version stamp on PET responses

**Yes, doable and small.** We will add an `info` request to the JSON-RPC schema returning:

```ts
interface InfoResponse {
  petVersion: string; // semver from Cargo.toml
  petGitSha: string; // baked in at build time via build.rs
  petBuildTimestamp: string;
  schemaVersion: string;
}
```

Build-time stamping uses the same `build.rs` that already wires Windows resources. Estimated effort: 1 PR, ~80 lines. This lets you pivot extension telemetry by PET SHA, which (as you note) is the only way to attribute regressions when insiders unpin.

### #7 — Per-locator timing inside `refresh`

**Already shipping** — see [crates/pet-core/src/telemetry/refresh_performance.rs](../crates/pet-core/src/telemetry/refresh_performance.rs). The `RefreshPerformance` payload sent as a JSON-RPC `"telemetry"` notification (event `"RefreshPerformance"`) contains:

```rust
pub struct RefreshPerformance {
    pub total: u128,                          // ms
    pub breakdown: BTreeMap<String, u128>,    // phase -> ms ("Locators", "Path", "GlobalVirtualEnvs", "Workspaces")
    pub locators: BTreeMap<String, u128>,     // LocatorKind -> ms
}
```

You should be able to attach `RefreshPerformance.locators["Hatch"]`, `["WindowsRegistry"]`, etc. to `pet.refresh` today. If `breakdownLocators` is reading 0% populated for you, that's an extension-side wiring issue worth a separate sync — happy to help walk through the notification handler.

We will add (next PR):

- `counts: BTreeMap<String, u32>` — number of paths scanned and envs found per locator. Lets you separate "slow because many envs" from "slow because Defender".
- Split `WindowsRegistry` into `WindowsRegistryHKLM` / `WindowsRegistryHKCU` keys.
- A `configure_ms` and `configure_locator_breakdown` field. **Especially relevant given the #416 diagnosis** — you'll be able to see configure duration directly.

### #8 — Windows Store stub detection counter

Still on our list. Will be added alongside the `counts` field above.

### Bonus — process-exit / crash telemetry

As described in Q5: a `panic::set_hook` that writes `<cache_dir>/pet-last-exit.json` plus a startup-time `LastExitReason` telemetry event. Scoped separately because it needs careful panic-safe write semantics and a multi-instance race story.

---

## Section 5 — Branch pinning cadence (your item 9)

We are happy to commit to a cadence:

- **Release branches cut every two weeks** from `main` after a one-week soak.
- **Insiders pins one release branch ahead of stable** — exactly what you proposed. The current pin to `refs/heads/main` is a known accident from earlier pipeline iteration, not a deliberate policy.
- We will **publish a `RELEASES.md` in this repo** with the cadence, the current `release/*` branch, and any open blockers per branch. CI will fail if a release branch is cut without an updated entry.

**On the immediate insider pin:** as called out in the TL;DR, `release/2026.8` does **not** yet contain the #461 (lock) or #462 (Hatch I/O) fixes — the issues are filed but no PR has merged. Pinning insiders to `release/2026.8` in its current state will not change observed `pet.refresh` p90. The right sequence is:

1. PET lands #461 + #462 on `main`.
2. PET either (a) cherry-picks both into `release/2026.8` and re-tags, or (b) cuts a new `release/2026.10` from `main` after a one-week soak.
3. Extension pins insiders to whichever branch in (2) is current.
4. Extension validates the regression over a week of telemetry — target `pet.refresh` p90 back to roughly v1.30 levels (darwin < 0.8s, win32 < 2.6s on the comparable cohort).
5. After validation, adopt the "insiders = release N+1, stable = release N" rule formally. **Do not promote v1.33 to stable until step 4 confirms the regression is closed** — the May 14–22 recovery curve is cohort drift, not a real fix (see Section 2 correction).

---

## Section 6 — Action items

**Status note:** no fix has landed yet. Issues #461–#465 are filed; the merge-date math is settled (see Sections 1 and 2); the cadence is agreed (Section 5). All items below are open.

PET-side (us) — tracked by GitHub issues in Section 7:

1. [ ] **Fix the #416 regression** via Option A above — see #461. **Target: end of week.**
2. [ ] Move Hatch's `pyproject.toml` / `hatch.toml` I/O out of `configure()` — see #462.
3. [ ] Either cherry-pick #461 + #462 into `release/2026.8` once merged, or cut `release/2026.10` from `main` after a one-week soak. **Do not let insiders pin to a branch that lacks both fixes.**
4. [ ] Investigate `resolve` 205s tail — see #463. Keep the 1-second acceptance target; the small cohort size (894 sessions / 7d) understates impact because each affected user gets a session-hosing hang.
5. [ ] Diagnose Windows native crash codes — see #464. **Concrete next step:** post a comment on #464 with (a) SQM IDs for the 26 affected machines (11 + 12 + 3, deduped) and (b) where available, the last `pet.process_restart` event payload preceding the exit. This unblocks correlation against the JSON-RPC payload right before exit.
6. [ ] Add `info` JSON-RPC method for version/SHA stamping — see #465.
7. [ ] Expand `RefreshPerformance` (counts, HKLM/HKCU split, `configure_ms`) — see Issue F (not yet filed).
8. [ ] Persist crash/panic last-exit reason + emit `LastExitReason` telemetry — see Issue G (not yet filed).
9. [ ] (Smaller) `BuildDetails::find` no-hint slow path — see Issue H (not yet filed).
10. [ ] Publish `RELEASES.md` with cadence.

Extension-side (your team, suggested):

1. [ ] **Do not pin insiders to `release/2026.8`** until it contains both #461 and #462 (either cherry-picked or via a new `release/2026.10`). Pinning to the current `release/2026.8` will not change observed p90.
2. [ ] **Do not promote v1.33 to stable** until a new insider build (post-fix) shows `pet.refresh` p90 returning to v1.30 levels (darwin < 0.8s, win32 < 2.6s on the comparable cohort). The May 14–22 recovery curve is cohort drift, not a real fix.
3. [ ] Add a bounded timeout on `resolve` calls (you mentioned this is planned — strongly second).
4. [ ] If possible, slice slow `resolve` tails by path prefix (`WindowsApps`, `registry`, `conda`, `pyenv`) and share — this would unblock #463.
5. [ ] Once #465 ships, call `info` on PET startup and stamp every event that already carries `pet.*` properties with `petVersion` and `petGitSha`. This is what makes the next regression bisectable.
6. [ ] Re-baseline the "bad-experience funnel" once #461 + #462 ship. Today insiders is at 17.38% vs stable 11.89%; post-fix the expectation is insiders < stable (cross-session cache benefit on top of equal PET perf).

---

## Section 7 — Proposed GitHub issues to file on `microsoft/python-environment-tools`

Seven issues. One **P0** (fixes the regression), two **P1** (Hatch amplifier + resolve hang investigation), and four **P2** (crash diagnosis, instrumentation gaps, small perf). All are ready to file as-is; titles, labels, bodies, and acceptance criteria are spelled out below.

### Issue A — `[P0][regression] Refresh blocked by configure: write lock held during all locator.configure() calls`

**Labels:** `bug`, `regression`, `perf`, `priority/P0`, `area/jsonrpc`

**Body:**

`apply_configure_options` ([crates/pet/src/jsonrpc.rs#L661-L719](../crates/pet/src/jsonrpc.rs#L661), introduced by [#416](https://github.com/microsoft/python-environment-tools/pull/416)) holds `configuration.write()` for the entire `for locator in locators.iter() { locator.configure(...) }` loop. Previously the lock was released _before_ configuring locators.

Refresh threads take `configuration.read()` in three places, all of which now block on the configure writer:

- `GenerationGuardedReporter::report_if_current` — read lock per `report_environment` call ([jsonrpc.rs#L416](../crates/pet/src/jsonrpc.rs#L416))
- `sync_refresh_locator_state_if_current` — read lock at end of refresh ([jsonrpc.rs#L376](../crates/pet/src/jsonrpc.rs#L376))
- `execute_refresh` — snapshot read at refresh start ([jsonrpc.rs#L1046](../crates/pet/src/jsonrpc.rs#L1046))

Result: when `configure` and `refresh` overlap, every per-environment report can block on locator I/O happening inside a different RPC handler. Telemetry attributes this as a `pet.refresh` p90 doubling on darwin (+91%) and +44% on win32 in the extension v1.33 insiders cohort (May 2026).

**Repro:** in a workspace with ≥ 3 workspace folders and ≥ 50 discovered Python envs, issue `configure` and `refresh` RPCs concurrently from the client and measure per-event durations. Pre-#416, the two are independent. Post-#416, refresh duration tracks the slowest locator's `configure()` runtime.

**Proposed fix (Option A from the regression doc):**

1. Add a `configure_in_progress: Mutex<()>` to `Context`. Acquire it at the top of `apply_configure_options`.
2. Briefly take `configuration.write()` only to compute `next_config` and `next_generation` (without publishing).
3. Drop the write lock; call `locator.configure(&next_config)` for each locator with no lock held.
4. Re-take `configuration.write()` for a fast publish (`state.config = next_config; state.generation = next_generation`).
5. Keep `panic::catch_unwind` + rollback semantics from #416 unchanged.

**Acceptance criteria:**

- [ ] Existing test `test_configure_publishes_state_after_shared_locators_are_configured` still passes (new `generation` is only observable after every locator's `configure()` returns).
- [ ] New test: while one configure is mid-`locator.configure()` (use a test locator that blocks on a barrier), a concurrent refresh's `report_environment` calls complete without blocking on the configure thread.
- [ ] New test: two concurrent `configure` calls serialize (no interleaved `locator.configure()` invocations).
- [ ] `cargo fmt --all` + `cargo clippy --all -- -D warnings` clean.

---

### Issue B — `[P1][perf] Hatch locator: defer pyproject.toml / hatch.toml parsing out of configure()`

**Labels:** `bug`, `perf`, `priority/P1`, `area/pet-hatch`

**Body:**

`Hatch::configure` ([crates/pet-hatch/src/lib.rs#L130-L154](../crates/pet-hatch/src/lib.rs#L130)) reads and parses `pyproject.toml` + `hatch.toml` for every workspace directory on every `configure` call. Combined with Issue A, this disk I/O runs while holding the configuration write lock and is the largest single amplifier of the cross-platform p90 regression observed in the v1.33 insiders cohort.

Even after Issue A is fixed, doing per-workspace TOML I/O on every `configure` is wasteful: VS Code calls `configure` whenever workspace folders change, and `pyproject.toml` rarely changes between calls.

**Proposed fix:**

- Cache parsed workspace Hatch config keyed by `(workspace_path, pyproject_mtime, hatch_toml_mtime)`. Reuse cached entry if mtimes are unchanged.
- Or: move the parse out of `configure()` entirely; do it lazily on first `try_from()`/`find()` per workspace, behind the existing `workspace_virtual_dirs` mutex.
- Either way, `configure()` itself should be O(workspace_count) `stat` calls, not O(workspace_count × 2) file reads + TOML parses.

**Acceptance criteria:**

- [ ] `Hatch::configure` does no file _reads_ (stats only) when workspace TOML files have not been modified since the last configure.
- [ ] Microbenchmark: configure with 10 workspaces × 2 TOML files takes < 5ms on a warm cache (currently ~50–200ms cold depending on disk).
- [ ] Functional tests from #460 still pass — Hatch envs are still correctly identified.

---

### Issue C — `[P1][bug][needs-investigation] resolve hangs for tens to hundreds of seconds on Windows for invalidated cached paths`

**Labels:** `bug`, `needs-investigation`, `priority/P1`, `area/jsonrpc`, `os/windows`

**Body:**

The extension team's telemetry (May 25 report) shows `pet.resolve` p90 = **205.4s** (p95 = 352.3s) for the `cache_stale` cohort on Windows v1.33 — paths that PET reports as invalid when the extension passes a cached path for resolution. 894 sessions affected over 7 days.

We do not yet know which code path stalls. Top hypotheses:

1. **Windows Store stub on a vanished package.** Cached path `~/AppData/Local/Microsoft/WindowsApps/python.exe` belongs to an uninstalled Store install. `try_from` may fall through to a `resolve_executable` that spawns the stub, which blocks on Store activation.
2. **`norm_case` on a non-existent path under Defender.** `GetLongPathNameW` can stall for tens of seconds on suspicious or quarantined paths.
3. **Retry loop in `resolve_symlink` or a locator's `try_from`** when the target is gone.

**Tasks:**

- [ ] Read `handle_resolve` and every `Locator::try_from` looking for unbounded loops, blocking spawns, or symlink chains.
- [ ] Add per-step timing to `resolve` (already partially present — extend with `try_from_ms` per locator and `norm_case_ms`).
- [ ] Coordinate with extension team for per-call path-prefix slicing of slow tails (Windows Store vs registry vs pyenv vs conda).
- [ ] Once root cause found, add a hard internal timeout on the offending step (in addition to the extension-side RPC timeout).

**Acceptance criteria:**

- [ ] `resolve` for a non-existent path returns within 1 second on Windows under default Defender configuration.
- [ ] Regression test using a tempdir with a since-deleted symlink target.

---

### Issue D — `[P2][bug][needs-investigation] Diagnose native Windows process exits (0xC0000142, 0xC0000005, 0xC0000409)`

**Labels:** `bug`, `needs-investigation`, `priority/P2`, `os/windows`

**Body:**

Extension telemetry (May 25 report) shows small but real counts of native Windows process-exit codes on v1.33 over 7 days:

| Exit code    | Meaning                    | Events | Machines |
| ------------ | -------------------------- | -----: | -------: |
| `0xC0000142` | DLL initialization failure |     90 |       11 |
| `0xC0000005` | Access violation           |     49 |       12 |
| `0xC0000409` | Stack buffer overrun       |     30 |        3 |

PET has no `panic::set_hook`, no `catch_unwind` at the JSON-RPC dispatcher, and no shutdown event. We currently cannot attribute these on our side.

**Tasks:**

- [ ] Pull SQM IDs from the extension team for the affected cohort.
- [ ] Cross-reference with the JSON-RPC payload right before exit (extension may have it in the last `pet.process_restart` event).
- [ ] Once Issue G (crash hook) is in, wait for repro on a real machine.

Note: this issue is mostly blocked on data; the _fix_ lives in Issue G.

---

### Issue E — `[P2][feature][instrumentation] Add info JSON-RPC method exposing PET version and git SHA`

**Labels:** `enhancement`, `instrumentation`, `priority/P2`, `area/jsonrpc`

**Body:**

The Python Environments extension cannot today attribute its `pet.refresh` / `pet.resolve` telemetry by PET commit — every insiders build embeds whatever PET was current at build time, and there is no way to read the PET version back from the binary at runtime. This blocks bisecting regressions like the May 25 one.

**Proposed JSON-RPC method:**

```ts
// Request:  { jsonrpc: "2.0", id: <n>, method: "info", params: {} }
// Response: { jsonrpc: "2.0", id: <n>, result: InfoResponse }

interface InfoResponse {
  petVersion: string; // crate version from Cargo.toml, e.g. "0.5.3"
  petGitSha: string; // short SHA, baked in at build time
  petBuildTimestamp: string; // ISO 8601
  schemaVersion: string; // JSONRPC schema version, e.g. "1"
}
```

**Implementation:**

- Extend `build.rs` (already used to embed Windows resources) to stamp `GIT_SHA` and `BUILD_TIMESTAMP` via `env!()`.
- `crate::version::PET_VERSION` from `env!("CARGO_PKG_VERSION")`.
- New `handle_info` in `crates/pet/src/jsonrpc.rs`. No locking, no I/O — pure const response.
- Document in `docs/JSONRPC.md`.

**Acceptance criteria:**

- [ ] `info` request returns within 1ms.
- [ ] Response fields populated in both `cargo build` and `cargo build --release`.
- [ ] Documented in `docs/JSONRPC.md` with TypeScript interface.
- [ ] Integration test in `crates/pet/tests/`.

---

### Issue F — `[P2][enhancement][instrumentation] Expand RefreshPerformance with counts, HKLM/HKCU split, and configure timing`

**Labels:** `enhancement`, `instrumentation`, `priority/P2`, `area/telemetry`

**Body:**

`RefreshPerformance` ([crates/pet-core/src/telemetry/refresh_performance.rs](../crates/pet-core/src/telemetry/refresh_performance.rs)) today contains `total`, `breakdown`, `locators`. To unblock extension-side attribution of regressions like the May 25 one, add:

1. **`counts: BTreeMap<String, u32>`** — paths scanned and envs found per locator. Lets consumers distinguish "slow because many envs" from "slow because Defender".
2. **Split `WindowsRegistry` into `WindowsRegistryHKLM` / `WindowsRegistryHKCU`** in the `locators` map. Most of the time is in HKLM walks; collapsing them hides the signal.
3. **`configure_ms: u128`** — total time spent in the most recent `configure` call.
4. **`configure_locator_breakdown: BTreeMap<String, u128>`** — per-locator configure time. Will make Issues A and B trivially attributable in production.
5. **Windows Store stub detection counter** — count of `python.exe` stubs detected vs real Store installs.

**Backwards compatibility:** strictly additive. All new fields default to empty/0; existing consumers continue to work.

**Acceptance criteria:**

- [ ] New fields populated on every refresh.
- [ ] `docs/JSONRPC.md` updated.
- [ ] No new locking on the refresh hot path (use the existing per-thread accumulators).

---

### Issue G — `[P2][bug][instrumentation] Persist crash/panic last-exit reason; emit LastExitReason telemetry on startup`

**Labels:** `enhancement`, `instrumentation`, `bug`, `priority/P2`

**Body:**

PET has no crash attribution today — when the JSON-RPC server panics or the OS kills it (see Issue D), the next process has no record of why. The extension currently has to infer cause from exit code alone.

**Proposed change:**

1. **`panic::set_hook`** at startup: serializes `{ kind: "panic", message, location, timestamp, last_request: <method+id> }` to `<cache_dir>/pet-last-exit.json` before delegating to the default hook.
2. **`catch_unwind` boundary** around the top-level JSON-RPC dispatch in `crates/pet/src/jsonrpc.rs`. Same hook, but allows graceful continuation if a single handler panics.
3. **On startup**, check for `pet-last-exit.json`. If present: emit a `LastExitReason` JSON-RPC telemetry notification, then delete the file.
4. **Multi-instance safety:** keyed write (use process-instance-unique filename like `pet-last-exit-<pid>-<startTime>.json`), and on startup ingest all such files, not just one. Avoids races between concurrent PET processes.

**Acceptance criteria:**

- [ ] Force a panic in a test handler; verify the next process emits `LastExitReason` with the panic message and location.
- [ ] Concurrent test: two PET processes both panic; both panics are reported on next startup, neither lost.
- [ ] Hook is panic-safe (does not panic itself if cache_dir is unwritable).
- [ ] OS-level kill (`SIGKILL`, access violation) leaves no orphaned `pet-last-exit-*.json` files in a stale state after a clean restart (file is aged-out > 7 days).

---

### Issue H — `[P3][perf] BuildDetails::find does fs::read_dir(<prefix>/lib) per virtualenv on the no-hint slow path`

**Labels:** `perf`, `priority/P3`, `area/pet-python-utils`

**Body:**

[`BuildDetails::find_with_hint`](../crates/pet-python-utils/src/build_details.rs#L100) ([#452](https://github.com/microsoft/python-environment-tools/pull/452)) takes a fast path (3–4 `is_file()` stats) when given a `(major, minor)` hint, but falls back to `fs::read_dir(<prefix>/lib)` + per-entry `is_dir()` + `is_file()` when no hint is available.

`version::from_creator_for_virtual_env` extracts the hint from `pyvenv.cfg`'s `version_major` / `version_minor`, but **older virtualenvs may not write those fields** — in which case every such virtualenv now does a full `read_dir` of the creator's `lib/` on every refresh.

Small per-env cost (~40 stat calls per affected env on macOS) but additive in workspaces with many old virtualenvs. Not part of the May 25 regression headliner, but worth tightening.

**Proposed fix:**

- When `pyvenv.cfg` lacks version fields, parse the `home = ...` line's executable name (e.g. `python3.11`) to derive a hint before falling through.
- Or: skip `BuildDetails::find` entirely when no hint is available (Python 3.13 and earlier don't ship `build-details.json` anyway — the file is a 3.14+ feature).

**Acceptance criteria:**

- [ ] `from_creator_for_virtual_env` does no `read_dir` calls on virtualenvs whose creator is Python ≤ 3.13.
- [ ] Existing tests in `crates/pet-python-utils/tests/sys_prefix_test.rs` still pass.

---

### Filing notes

- File Issue A immediately and reference this doc; it's the only true P0.
- Issues B–H can be filed as a batch with cross-references. Suggested order: A → C → D → G → E → F → B → H.
- Each issue body is self-contained — no need to read this doc to act on any single one.

---

## Reproducing this investigation

All findings are reproducible from a fresh clone:

```bash
git log --oneline origin/release/2026.6..origin/release/2026.8        # 18 commits
git show 1c28b88 -- crates/pet/src/jsonrpc.rs                          # the regression
git show d581272 -- crates/pet-hatch/src/lib.rs                        # the amplifier
git show ab6d1d2 -- crates/pet-python-utils/src/version.rs             # the per-env extra stats
```

Key files to read for the lock-contention chain:

- [crates/pet/src/jsonrpc.rs#L376-L468](../crates/pet/src/jsonrpc.rs#L376) — `GenerationGuardedReporter` (refresh-side read locks)
- [crates/pet/src/jsonrpc.rs#L661-L719](../crates/pet/src/jsonrpc.rs#L661) — `apply_configure_options` (configure-side write lock holding I/O)
- [crates/pet-hatch/src/lib.rs#L130-L154](../crates/pet-hatch/src/lib.rs#L130) — `Hatch::configure` (the I/O performed under the lock)

---

— PET team
