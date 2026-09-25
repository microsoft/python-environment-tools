# JSONPRC Messages

The tool supports JSONRPC messages for communication.
The messages are sent over a stdio/stdout. The messages are in the form of a JSON object.

The messages/notifications supported are listed below.

This document assumes the reader is familiar with the JSONRPC 2.0 specification.
Hence there's no mention of the `jsonrpc` property in the messages.
For samples using JSONRPC, please have a look at the [sample.js](./sample.js) file.

Any requests/notifications not documented here are not supported.

## Transport lifetime and limits

Close the server process's stdin after receiving all responses you need. EOF at a
frame boundary is a normal shutdown, not an empty request. PET stops accepting
requests and output, discards pending notifications/replies, cancels admitted
interpreter and manager probes, and exits successfully once their ownership
cleanup finishes. Closing stdin is cancellation, not a request to drain unfinished
requests. Test clients wait for normal exit and forcibly terminate only their own
child as a bounded failure fallback.

EOF inside a header or payload, malformed framing, and read/write/flush failures
are terminal errors: PET reports the failure on stderr and exits unsuccessfully.
Malformed JSON in a complete frame instead receives a Parse Error (`-32700`,
`id: null`), after which subsequent frames can still be processed. Protocol stdout
contains framed JSONRPC only.

Input is currently one `Content-Length` header followed by a blank line and the
specified number of UTF-8 payload bytes. Both CRLF and LF line endings are accepted.
Headers including the separator are limited to 8 KiB, and payloads to 16 MiB, before
payload allocation. Multi-header input parsing is tracked separately in
[#532](https://github.com/microsoft/python-environment-tools/issues/532).

One process-lifetime writer emits accepted frames in FIFO order, so a refresh reply
cannot overtake notifications already admitted before it. Each serialized output
payload is limited to 16 MiB; the queue holds at most 1,024 frames and 32 MiB of
retained frame capacity. The queue limit excludes one in-flight frame and the
single producer's serialization/frame-building buffers, whose payloads are also
limited to 16 MiB each. Queue saturation is a terminal connection error rather
than an unbounded allocation or a wait on a slow consumer. These are output bounds,
not a bound on total request/discovery memory or worker concurrency.

The writer owns a duplicate OS stdout handle and does not hold Rust's global stdout
lock. Shutdown drops queued output and abandons in-flight output without joining
an OS-blocked writer or stdin reader; those process-lifetime threads end when the
standalone server exits. The first output failure recorded before closure remains
fatal; errors arriving after normal closure are discarded with the cancelled work.
Admitted probes are tracked separately through ownership cleanup, with a shared
three-second server shutdown wait. Cleanup failures or an expired wait are reported
as errors, never as clean shutdown. Synchronous OS process creation and individual
OS I/O calls cannot be interrupted by this mechanism; nor does it extend the Unix
ownership boundary to descendants that deliberately escape their process group.

## Request identifiers

Requests include an `id` that is a string, JSON number, or explicit `null`. PET preserves
the parsed value in successful replies and errors, including every waiter joining a refresh.
Signed 64-bit and unsigned 64-bit integer IDs are preserved without narrowing to 32 bits.
Fractional numbers are accepted using the JSON parser's floating-point representation; numeric
spelling is not preserved. Prefer string IDs when exact values exceed the 64-bit integer ranges
or the precision of a client's numeric type. JSONRPC recommends avoiding fractional and null IDs.

Only an absent `id` denotes a notification. Boolean, array, and object IDs produce an Invalid
Request error (`-32600`) with `id: null` and do not invoke a handler. Other existing method and
parameter error codes are unchanged. Notifications do not receive request replies.

# Info Request

Returns metadata about the running PET binary. This request does not require a prior
`configure` request. Clients can cache this response and attach it to PET-related
telemetry such as `refresh` and `resolve` timings.

_Request_:

- method: `info`
- params: `{}`

_Response_:

- result: `InfoResponse` defined as below.

```typescript
interface InfoResponse {
  /**
   * PET package version baked into the binary at build time.
   * Pre-release builds may include a suffix such as `0.1.0-dev.12345`.
   */
  petVersion: string;
  /**
   * Build identifier baked into the binary when built by CI.
   * Sourced from `PET_BUILD_ID` or Azure Pipelines `BUILD_BUILDID`.
   */
  buildId?: string;
  /**
   * Source git commit SHA baked into the binary when built by CI.
   * Sourced from `PET_COMMIT_SHA`, Azure Pipelines `BUILD_SOURCEVERSION`,
   * or GitHub Actions `GITHUB_SHA`. Absent for local dev builds.
   */
  commitSha?: string;
}
```

# Configuration Request

This should always be the first request sent to the tool.
This request should be sent again, only if any of the configuration options change.

The request is expected to contain the configuraiton information for the tool to use.
All properties of the configuration are optional.

_Request_:

- method: `configure`
- params: `ConfigureParams` defined as below.

_Response_:

- result: `null`

```typescript
interface ConfigureParams {
  /**
   * This is a list of project directories.
   * Useful for poetry, pipenv, virtualenvwrapper and the like to discover virtual environments that belong to specific project directories.
   * E.g. `workspace folders` in vscode.
   *
   * If not provided, then environments such as poetry, pipenv, and the like will not be reported.
   * This is because poetry, pipenv, and the like are project-specific environments.
   *
   * Glob patterns are supported (e.g., `/home/user/projects/*`). Avoid recursive `**` patterns when a single-level pattern is sufficient.
   * Expansion is limited to 1,024 distinct brace-expanded patterns and 10,000
   * filesystem candidates per configured field. Invalid patterns, traversal failures, and
   * exceeded limits fail the configure request; PET does not apply a partial configuration.
   */
  workspaceDirectories?: string[];
  /**
   * This is a list of directories where we should look for python environments such as Virtual Environments created/managed by the user.
   * This is useful when the virtual environments are stored in some custom locations.
   *
   * Useful for VS Code so users can configure where they store virtual environments.
   *
   * Values identify directories that contain environments. Glob patterns are supported (e.g., `<root>/envs`, `<root>/*/envs`).
   * Avoid recursive patterns such as `<root>/**/envs`: they can traverse large directory trees.
   * The same 1,024-pattern and 10,000-candidate limits apply to these patterns.
   */
  environmentDirectories?: string[];
  /**
   * This is the path to the conda executable.
   *
   * Useful for VS Code so users can configure where they have installed Conda.
   * For backwards compatibility, this can also be a path to a mamba or micromamba executable.
   */
  condaExecutable?: string;
  /**
   * This is the path to the pipenv executable.
   *
   * Useful for VS Code so users can configure where they have installed Pipenv.
   */
  pipenvExecutable?: string;
  /**
   * This is the path to the poetry executable.
   *
   * Useful for VS Code so users can configure where they have installed Poetry.
   */
  poetryExecutable?: string;
  /**
   * Directory to cache Python environment details.
   * WARNING: This directory will be deleted in the `clearCache` request.
   * It is advisable to use a directory that is not used by other tools, instead have a dedicated directory just for this tool.
   *
   * Data in this directory can be deleted at any time by the client.
   */
  cacheDirectory?: string;
}
```

# Refresh Request

Performs a refresh/discovery of Python environments and reports them via `environment` and `manager` notifications.
All properties of the configuration are optional.

_Request_:

- method: `refresh`
- params: `RefreshParams` defined as below.

_Response_:

- result: `RefreshResult` defined as below.

```typescript
interface RefreshParams {
  /**
   * Limits the search to a specific kind of Python environment.
   * Workspace folders from the configuration request are still searched
   * to discover workspace-based environments (e.g., venvs, virtualenvs).
   */
  searchKind?: PythonEnvironmentKind;
} | {
  /**
   * Limits the search to a specific set of paths.
   * searchPaths can either by directories or Python prefixes/executables or combination of both.
   * Replaces workspace folders from the configuration request.
   *
   * Glob patterns are supported:
   * - `*` matches any sequence of characters in a path component
   * - `?` matches any single character
   * - `**` matches any sequence of path components (recursive)
   * - `[...]` matches any character inside the brackets
   * - `{a,b}` matches either `a` or `b` (brace expansion)
   *
   * Examples:
   * - "/home/user/projects/*" - all directories under projects
   * - "/home/user/**/venv" - all venv directories recursively
   * - "/home/user/project[0-9]" - project0, project1, etc.
   * - "./**/{bin,Scripts}/python{,.exe}" - Python executables in bin or Scripts dirs
   */
  searchPaths?: string[];
}

interface RefreshResult {
  /**
   * Discovery-engine duration in milliseconds. This is the server's find-and-report interval.
   * It excludes request parsing and glob expansion, coordinator queueing, locator
   * preparation, reply delivery, and post-discovery synchronization. Clients needing
   * operation latency must measure through receipt of this response.
   */
  duration: number;
  /**
   * Identifier shared by this result and all RefreshProgress telemetry emitted
   * for the refresh operation. Concurrent identical requests that join the same
   * operation receive the same identifier.
   */
  refreshId: number;
}
```

`searchPaths` are parsed on the transport thread but expanded in the refresh worker,
so filesystem traversal does not delay dispatch of unrelated requests such as `info`.
Duplicate input patterns and duplicate expanded paths are searched once. Concurrent
refreshes with the same normalized expanded paths, options, and configuration generation
join one operation; different options or generations do not.

Windows glob matching preserves the pinned matcher rules: verbatim disk paths such
as `\\?\C:\envs\*` are supported, while other verbatim prefixes (including
`\\?\UNC\...`) produce no matches without filesystem traversal. Invalid patterns
still fail syntax validation before that prefix rule is applied. Literal components,
including names containing a lone `]`, retain normal filesystem lookup semantics.

Expansion allows at most 1,024 distinct brace-expanded patterns and 10,000
filesystem candidates per request. Brace expansion also stops after 10,000 work steps
per input pattern, counting each pending pattern and each alternative before formatting
or deduplication (including duplicate alternatives). Invalid patterns, traversal failures, or exceeded
limits return a JSON-RPC error (`-4`) and no partial refresh inventory. Limits are
checked between filesystem entries; they are not a timeout and cannot interrupt an
operating-system filesystem call already in progress.

PET admits at most two configure/refresh glob or brace expansions concurrently.
Requests containing only literal paths do not consume these slots. Additional expansion
requests receive JSON-RPC error `-4` instead of creating an unbounded traversal queue.
Input deduplication preserves the first occurrence order of configured paths.

### Refresh timing boundaries

The quality benchmark records separate clocks. **Refresh round-trip** starts immediately
before the client serializes and writes `refresh` and ends after its matching response is
read. It includes parsing and glob expansion, coordinator waiting, discovery, synchronization
before the reply, and transport. Inventory/progress clearing and diagnostics are outside it.

**Request-to-first environment** uses the same request boundary and ends at the first
`environment` notification read before the matching response; its client-side observation resets
for every refresh and closes on its response or error. Notifications processed by a later
non-refresh request cannot populate the completed refresh's missing observation.
**Startup-to-first environment** starts immediately before PET is spawned, ends at the first
`environment` notification in that process, and never resets.

Environment notifications do not carry a request or `refreshId`, so clients cannot attribute
concurrent or post-response environment notifications to a particular refresh. A queued request
includes coordinator waiting, and identical concurrent requests can coalesce and share a
`refreshId`, but request-to-first remains a sequential client observation rather than a protocol
correlation guarantee. The quality workload avoids that ambiguity by using a fresh process for each
single measured refresh, requires one sample per operation, and never silently omits a missing
sample.

## Refresh Progress Telemetry

During a refresh, the server emits `telemetry` notifications when each major phase
and locator starts and completes. These notifications contain timing and enum values
only; they never contain paths, environment names, executable paths, usernames, or
command lines.

```typescript
interface RefreshProgressTelemetry {
  event: "RefreshProgress";
  data: {
    refreshProgress: {
      refreshId: number;
      phase: "locators" | "path" | "globalVirtualEnvs" | "workspaces";
      status: "started" | "completed";
      elapsedMs: number;
      phaseElapsedMs?: number;
      locatorName?: string;
      locatorElapsedMs?: number;
    };
  };
}
```

`phaseElapsedMs` is present for completed phases. Locator events use the `locators`
phase and include `locatorName`; completed locator events also include
`locatorElapsedMs`.

# Subprocess Probe Lifecycle

Interpreter probes used for resolution and manager probes used during refresh/discovery
(Conda info, Poetry environment lists and Poetry configuration) share these limits:

- Both stdout and stderr are drained while the direct child runs. At most 4 MiB combined is retained (a captured-byte limit, not a total memory limit). Excess output, a nonzero exit, and I/O failures are logged and treated as failed probes.
- Each execution deadline is 15 seconds after synchronous OS process creation returns. On failure or direct-child exit, PET terminates the owned process group/job and reaps the direct child, allowing one shared allowance of up to 2 additional seconds for cleanup. Exceptional OS cleanup failures retain the primary error and attempt background direct-child reaping. If resource exhaustion also prevents starting that waiter, the failure and child PID are logged; reaping cannot be guaranteed in that exceptional case.
- Unix probes start in a new process group. PET signals that group before reaping its leader, avoiding process-ID reuse. On macOS, an otherwise ambiguous permission error is accepted only when bounded, unchanged membership and process-birth identities confirm that all group members are already zombies; incomplete or failed inspection remains an error. Descendants that deliberately start another group/session escape this boundary; PET does not act as a system-wide descendant reaper.
- Windows probes start suspended and without a console window. PET assigns an unnamed, non-breakaway, kill-on-close job before resuming the child's sole thread. Stable Rust does not expose the primary-thread handle, so PET uses a per-process thread-metadata snapshot (`PssCaptureSnapshot`, Windows 8.1+) and verifies the selected thread's process identity. It does not enumerate system-wide threads or clone the child's address space. Ambiguous/missing thread ownership or failed job assignment fails the probe rather than running it unsupervised. Command arguments, environment, working directory, and batch-file launch still use Rust's standard process implementation; the discovery-only runner replaces any caller-supplied Windows creation flags with `CREATE_NO_WINDOW | CREATE_SUSPENDED`. All current interpreter/manager callers previously supplied only `CREATE_NO_WINDOW`.
- After the direct child exits, draining continues within the same deadline. If output has not reached EOF (for example, an escaped Unix descendant retains a write handle), the probe fails explicitly with incomplete output rather than waiting indefinitely. Output produced only by background helpers after their parent exits is not guaranteed: remaining owned helpers are terminated, including on successful parent exit.
- These are per-subprocess limits, not a total request/workspace budget. They do not bound synchronous OS process creation. During server shutdown, new probes are refused and active probes are cancelled through the same ownership cleanup. Cancellation alone is traced rather than logged as a probe failure; cleanup failures remain errors.

Missing default Conda executables are quietly ignored; installed/custom manager failures and all
timeouts/output failures are logged. Interpreter and Conda JSON is parsed strictly. Poetry stdout
must be UTF-8, and boolean configuration accepts only `true`, `false`, or unset `null`. Invalid
JSON, encoding or boolean values fail explicitly. Poetry path parsing retains its existing textual
contract: each nonempty environment-list line is treated as a path after trimming an activated
suffix; configuration paths use trimmed output. This does not claim structural/path-existence
validation of arbitrary UTF-8 prose or configuration path text.

# Resolve Request

Use this request to resolve a Python environment from a given Python path.

**Notes:**

- This request will generally end up spawning the Python process to get the environment information.
  Hence it is advisable to use this request sparingly and rely on Python environments being discovered or relying on the information returned by the `refresh` request.
- Interpreter probes follow the [shared subprocess limits](#subprocess-probe-lifecycle) above.
- If the `cacheDirectory` has been provided and the same python executable was previously spanwed (resolved), then the tool will return the cached information.

_Why use this over the `refresh` request?_

Some of the information in the Python environment returned as a result of the `refresh` request might not be available is not available in the `Environment` object.
For instance sometimes the `version` and `prefix` can be empty.
In such cases, this `resolve` request can be used to get this missing information.

_Request_:

- method: `resolve`
- params: `ResolveParams` defined as below.

_Response_:

- result: `Environment` defined as below.

```typescript
interface ResolveParams {
  /**
   * The fully qualified path to the Pyton executable.
   */
  executable: string;
}

enum PythonEnvironmentKind {
  Conda,
  Pixi,
  Homebrew,
  Pyenv,
  GlobalPaths, // Python found in global locations like PATH, /usr/bin etc.
  PyenvVirtualEnv, // Pyenv virtualenvs.
  Pipenv,
  Poetry,
  MacPythonOrg, // Python installed from python.org on Mac
  MacCommandLineTools,
  LinuxGlobal, // Python installed in Linux in paths such as `/usr/bin`, `/usr/local/bin` etc.
  MacXCode,
  Uv,
  UvWorkspace,
  Venv,
  VirtualEnv,
  VirtualEnvWrapper,
  WinPython, // WinPython portable distribution for Windows
  WindowsStore,
  WindowsRegistry,
}

interface Environment {
  /**
   * The display name of the enviornment.
   * Generally empty, however some tools such as Windows Registry may provide a display name.
   */
  disdplay_name?: string;
  /**
   * The name of the envirionment.
   * Generally empty, however some tools such as Conda may provide a display name.
   * In the case of conda, this is the name of the conda environment and is used in activation of the conda environment.
   */
  name?: string;
  /**
   * The fully qualified path to the executable of the envirionment.
   * Generally non-empty, however in the case of conda environmentat that do not have Python installed in them, this may be empty.
   *
   * Some times this may not be the same as the `sys.executable` retured by the Python runtime.
   * This is because this path is the shortest and/or most user friendly path to the Python executable.
   * For instance its simpler for users to remember and use /usr/local/bin/python3 as opposed to /Library/Frameworks/Python.framework/Versions/Current/bin/python3
   *
   * All known symlinks to the executable are returned in the `symlinks` property.
   */
  executable?: string;
  /**
   * The kind of the environment.
   */
  kind?: PythonEnvironmentKind;
  /**
   * The version of the python executable.
   * This will at a minimum contain the 3 parts of the version such as `3.8.1`.
   * Somtime it might also contain other parts of the version such as `3.8.1+` or `3.8.1.final.0`
   */
  version?: string;
  /**
   * The prefix of the Python environment as returned by `sys.prefix` in the Python runtime.
   */
  prefix?: string;
  /**
   * The bitness of the Python environment.
   */
  arch?: "x64" | "x86";
  /**
   * The list of known symlinks to the Python executable.
   * Note: These are not all the symlinks, but only the known ones.
   * & they might not necessarily be symlinks as known in the strict sense, however they are all the known executables that point to the same Python Environment.
   *
   * E.g. the exes <sys prefix>/bin/python and <sys prefix>/bin/python3 are symlinks to the same Python environment.
   */
  symlinks?: string[];
  /**
   * The project folder this Python environment belongs to.
   * Poetry, Pipenv, Virtualenvwrapper and the like are project specific environments.
   * This is the folder where the project is located.
   */
  project?: string;
  /**
   * The associated manager.
   * E.g. `poetry`, `conda`, `pyenv` and the like.
   *
   * Even if a conda environment is discovered, the manager can still be empty.
   * This happens when we're unable to determine the manager associated with the environment.
   *
   * Note, just because this tool discoveres other conda environments and they all have managers associated with them, it does not mean that we can use the same manager for this environment when not know.
   * Thats because there could be multiple conda installations on the system, hence we try not to make any assumptions.
   */
  manager?: Manager;
  /**
   * An error message if the environment is known to be in a bad state.
   * For example: "Python executable is a broken symlink"
   * If undefined, no known issues have been detected (but this doesn't guarantee
   * the environment is fully functional - we don't spawn Python to verify).
   */
  error?: string;
}

interface Manager {
  /**
   * The fully qualified path to the executable of the manager.
   * E.g. fully qualified path to the conda exe.
   */
  executable: string;
  /**
   * The type of the Manager.
   */
  tool: "Conda" | "Mamba" | "Pipenv" | "Poetry" | "Pyenv";
  /**
   * The version of the manager/tool.
   * In the case of conda, this is the version of conda.
   */
  version?: string;
}
```

# Clear Cache Request

Use this request to clear the cache that the tool uses to store Python environment details.

**Notes:**

- This is a noop, if a `cacheDirectory` has not been provided in the `configure` request.

**Warning:**

- The directory provided in the `cacheDirectory` in the `configure` request will be deleted.
  Hence it is advisable to use a directory that is not used by other tools, instead have a dedicated directory just for this tool.

_Request_:

- method: `find`
- params: `null`

_Response_:

- result: `null`

# Log Notification

Sent by the server to log messages

_Notification_:

- method: `resolve`
- params: `LogParams` defined as below.

```typescript
interface LogParams {
  /**
   * The level of the log message.
   */
  level: "info" | "warning" | "error" | "debug" | "trace";
  /**
   * Message to log.
   */
  message: string;
}
```

# Manager Notification

Sent by the server whenever an Environment Manager is discovered.

_Notification_:

- method: `manager`
- params: `Manager` defined earlier.

# Environment Notification

Sent by the server whenever an Environment is discovered.

_Notification_:

- method: `environment`
- params: `Environment` defined earlier.
