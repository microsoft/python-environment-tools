# Locator Request And Refresh State

The JSONRPC server atomically publishes one immutable request snapshot containing the configuration, generation, and locator graph. Find, resolve, refresh, and Conda telemetry clone that snapshot under a short read lock and perform all filesystem work, subprocess work, and callbacks after releasing the lock. A configure request is serialized with other configures, prepares and configures its replacement graph off-lock, and publishes the complete replacement in one write. A failed replacement is never published, so the previous snapshot remains usable without mutating it back through rollback.

Requests that started before publication retain their old `Arc` snapshot and complete coherently against it. Generation-guarded refresh notifications use the snapshot's active gate: a notification that committed before publication may finish, while later notifications from the retired generation are suppressed without holding the published-state lock during the callback.

Refresh requests run on a transient locator graph. The server configures that graph from its captured request snapshot, runs discovery, and then syncs selected refresh-discovered state back into that snapshot's locator graph only if its generation is still current.

The `Locator::refresh_state()` classification is the contract for that boundary. It keeps configured inputs, self-hydrating caches, and correctness-critical discovery state distinct.

## Locator Lifetimes Across Configure

Configuration-independent locators are reused by `Arc`, rather than copied, when a replacement graph is built. This preserves cache and discovery updates that overlap configuration without a copy/publication window that could lose updates. The reused set is Windows Store, Windows Registry, WinPython, PyEnv, Pixi, VirtualEnvWrapper, Venv, VirtualEnv, Homebrew, MacXCode, MacCommandLineTools, MacPythonOrg, and LinuxGlobal.

Conda is reused, including its discovery state, only when `condaExecutable` is unchanged. When that input changes, the replacement gets fresh discovery state but shares Conda's fingerprint-validated environment-information cache. PyEnv and Windows Registry capture Conda as a dependency, so they are rebound to the replacement Conda instead of retaining their old locator Arcs. Rebinding preserves PyEnv's manager/version-directory cache and Windows Registry's cached registry walk; subsequent nested discovery and cached registry replay register environments with the replacement Conda rather than the retired snapshot. Poetry is reused when both its workspace directories and executable are unchanged, preserving synchronized manager, project, and environment fidelity; changed Poetry inputs get a fresh locator. Uv, PipEnv, and Hatch are always fresh and configured from the replacement snapshot. Hatch therefore retains its existing configure boundary: workspace inputs change and parsed project state is invalidated rather than copied.

The resolved-interpreter disk cache is deliberately process-wide performance state, not snapshot-local locator state. Its directory is write-once for the process: the first configured path remains effective, later path changes retain the existing warning, and published configurations record that first effective path while still accepting the request's other valid configuration changes. First initialization occurs before replacement-graph construction and snapshot publication, so an old in-flight request may overlap initialization of the global cache without observing partially configured locators. A later configuration failure still leaves the previous request snapshot usable; it does not roll back this independent process-lifetime cache initialization.

## Find Configuration Lock Scope

A directory `find` request copies `environmentDirectories` into an owned local snapshot before workspace discovery. The configuration read lock is released before filesystem traversal or locator identification, so a slow find does not keep configuration publication waiting on that lock. The active search continues using its captured directory list if configuration changes while discovery is running. Executable-only find requests do not read this list.

Executable and directory finds use the locator graph from the same captured request snapshot as the directory list. Resolve uses one captured graph for its complete locator traversal.

## Classifications

| Classification         | Meaning                                                                                           | Sync behavior                                                                                                          |
| ---------------------- | ------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `Stateless`            | The locator keeps no mutable state that survives a request.                                       | Nothing is copied back.                                                                                                |
| `ConfiguredOnly`       | The locator stores configured inputs such as executable paths or workspace directories.           | Refresh must use the transient locator's request snapshot and must not copy this state back.                           |
| `SelfHydratingCache`   | The locator stores a cache that later requests can rebuild on demand.                             | Refresh may fill a transient cache, but correctness must not rely on syncing it.                                       |
| `SyncedDiscoveryState` | The locator stores refresh-discovered state that later requests need for correctness or fidelity. | The locator must override `sync_refresh_state_from()` and copy only state appropriate for the `RefreshStateSyncScope`. |

## Current Locator Inventory

| Locator             | Mutable state                                                                   | Classification         | Notes                                                                                                                       |
| ------------------- | ------------------------------------------------------------------------------- | ---------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| WindowsStore        | Discovered Store environments                                                   | `SyncedDiscoveryState` | Full and matching global-kind refreshes replace the cache; workspace refreshes leave it alone.                              |
| WindowsRegistry     | Discovered registry managers and environments                                   | `SyncedDiscoveryState` | Full and matching global-kind refreshes replace the cache; workspace refreshes leave it alone.                              |
| WinPython           | Discovered WinPython environments                                               | `SyncedDiscoveryState` | Full and matching global-kind refreshes replace the cache; workspace refreshes leave it alone.                              |
| PyEnv               | Manager and versions-directory cache                                            | `SelfHydratingCache`   | `find()` clears the cache, and `try_from()` can rebuild it from the environment.                                            |
| Pixi                | None                                                                            | `Stateless`            | Identification is derived from filesystem markers.                                                                          |
| Conda               | Environment, manager, and mamba-manager discovery caches; configured executable | `SyncedDiscoveryState` | Discovery caches are synced. Transient refresh locators share an mtime-keyed environment-info cache with the long-lived locator; configured executable state remains request-local. |
| Uv                  | Configured workspace directories; immutable uv install directory                | `ConfiguredOnly`       | Workspace directories come from the request configuration snapshot.                                                         |
| Poetry              | Configured workspace directories and executable; discovered search result       | `SyncedDiscoveryState` | Search results are synced or merged by scope. Configured inputs are not copied back.                                        |
| PipEnv              | Configured pipenv executable                                                    | `ConfiguredOnly`       | The executable comes from the configuration snapshot.                                                                       |
| Hatch               | Configured workspaces and lazily parsed project configuration                   | `ConfiguredOnly`       | A fresh configured locator invalidates parsed project state for each published generation.                                  |
| VirtualEnvWrapper   | Environment variables captured at construction                                  | `Stateless`            | No refresh-discovered mutable state.                                                                                        |
| Venv                | None                                                                            | `Stateless`            | Identification is derived from `pyvenv.cfg` and filesystem layout.                                                          |
| VirtualEnv          | None                                                                            | `Stateless`            | Identification is derived from virtualenv markers.                                                                          |
| Homebrew            | Environment variables captured at construction                                  | `Stateless`            | No refresh-discovered mutable state.                                                                                        |
| MacXCode            | None                                                                            | `Stateless`            | macOS-only locator.                                                                                                         |
| MacCommandLineTools | None                                                                            | `Stateless`            | macOS-only locator.                                                                                                         |
| MacPythonOrg        | None                                                                            | `Stateless`            | macOS-only locator.                                                                                                         |
| LinuxGlobal         | Reported executable cache                                                       | `SelfHydratingCache`   | `try_from()` can repopulate the cache by scanning known global bin directories.                                             |

## Clear Semantics

The JSONRPC `clear` request clears only the process-wide resolved-interpreter cache. It does not clear locator discovery caches or replace the published request snapshot. In-flight operations continue with the snapshot they captured; later operations capture the current snapshot. An entry obtained before `clear` may complete a persistent write afterward and repopulate the cache; subsequent requests can use that entry. This is intentional performance-cache lifetime behavior and does not change the server generation or locator discovery state.

## Updating The Contract

When adding mutable state to a locator, classify it before relying on it across refreshes:

1. If it is configured input, keep it under `ConfiguredOnly` and source it from `Configuration`.
2. If it is only a performance cache, use `SelfHydratingCache` and make later requests able to rebuild it.
3. If later requests need refresh-discovered state, use `SyncedDiscoveryState`, implement `sync_refresh_state_from()`, and cover full, workspace, and kind-filtered scopes with tests.

The locator graph has a regression test in `crates/pet/src/jsonrpc.rs` that pins the current classification of each locator created by `create_locators()`.

## Module Ownership

The CLI and JSONRPC server both use the `pet` library's public `find` and `locators` modules. The binary owns CLI dispatch and its `jsonrpc` adapter, rather than compiling separate copies of discovery and locator code. Discovery/locator unit tests run under the library target; JSONRPC orchestration tests remain under the binary target. This module boundary does not change the transient and shared locator lifetimes described above.
