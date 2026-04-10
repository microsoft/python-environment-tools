# Locator Refresh State

Refresh requests run on a transient locator graph. The server configures that graph from a single configuration snapshot, runs discovery, and then syncs selected refresh-discovered state back into the long-lived shared locator graph only if that configuration generation is still current.

The `Locator::refresh_state()` classification is the contract for that boundary. It keeps configured inputs, self-hydrating caches, and correctness-critical discovery state distinct.

## Classifications

| Classification | Meaning | Sync behavior |
| --- | --- | --- |
| `Stateless` | The locator keeps no mutable state that survives a request. | Nothing is copied back. |
| `ConfiguredOnly` | The locator stores configured inputs such as executable paths or workspace directories. | Refresh must use the transient locator's request snapshot and must not copy this state back. |
| `SelfHydratingCache` | The locator stores a cache that later requests can rebuild on demand. | Refresh may fill a transient cache, but correctness must not rely on syncing it. |
| `SyncedDiscoveryState` | The locator stores refresh-discovered state that later requests need for correctness or fidelity. | The locator must override `sync_refresh_state_from()` and copy only state appropriate for the `RefreshStateSyncScope`. |

## Current Locator Inventory

| Locator | Mutable state | Classification | Notes |
| --- | --- | --- | --- |
| WindowsStore | Discovered Store environments | `SyncedDiscoveryState` | Full and matching global-kind refreshes replace the cache; workspace refreshes leave it alone. |
| WindowsRegistry | Discovered registry managers and environments | `SyncedDiscoveryState` | Full and matching global-kind refreshes replace the cache; workspace refreshes leave it alone. |
| WinPython | None | `Stateless` | Windows-only locator. |
| PyEnv | Manager and versions-directory cache | `SelfHydratingCache` | `find()` clears the cache, and `try_from()` can rebuild it from the environment. |
| Pixi | None | `Stateless` | Identification is derived from filesystem markers. |
| Conda | Environment, manager, and mamba-manager discovery caches; configured executable | `SyncedDiscoveryState` | Discovery caches are synced. The configured executable remains configuration state and is not copied from refresh locators. |
| Uv | Configured workspace directories; immutable uv install directory | `ConfiguredOnly` | Workspace directories come from the request configuration snapshot. |
| Poetry | Configured workspace directories and executable; discovered search result | `SyncedDiscoveryState` | Search results are synced or merged by scope. Configured inputs are not copied back. |
| PipEnv | Configured pipenv executable | `ConfiguredOnly` | The executable comes from the configuration snapshot. |
| VirtualEnvWrapper | Environment variables captured at construction | `Stateless` | No refresh-discovered mutable state. |
| Venv | None | `Stateless` | Identification is derived from `pyvenv.cfg` and filesystem layout. |
| VirtualEnv | None | `Stateless` | Identification is derived from virtualenv markers. |
| Homebrew | Environment variables captured at construction | `Stateless` | No refresh-discovered mutable state. |
| MacXCode | None | `Stateless` | macOS-only locator. |
| MacCommandLineTools | None | `Stateless` | macOS-only locator. |
| MacPythonOrg | None | `Stateless` | macOS-only locator. |
| LinuxGlobalPython | Reported executable cache | `SelfHydratingCache` | `try_from()` can repopulate the cache by scanning known global bin directories. |

## Updating The Contract

When adding mutable state to a locator, classify it before relying on it across refreshes:

1. If it is configured input, keep it under `ConfiguredOnly` and source it from `Configuration`.
2. If it is only a performance cache, use `SelfHydratingCache` and make later requests able to rebuild it.
3. If later requests need refresh-discovered state, use `SyncedDiscoveryState`, implement `sync_refresh_state_from()`, and cover full, workspace, and kind-filtered scopes with tests.

The locator graph has a regression test in `crates/pet/src/jsonrpc.rs` that pins the current classification of each locator created by `create_locators()`.