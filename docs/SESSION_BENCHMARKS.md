# Long-lived session benchmarks

`session_performance` runs deterministic virtual-environment fixtures against
real PET servers. For every inventory size it labels and measures three distinct
file-only refresh scenarios: a first server process with an explicitly empty
cache directory, a new process after that first refresh using the same cache
directory, and repeated warm refreshes in that second process. Process-cold does
not imply an OS-cold filesystem cache. These fake environments are identified
from files and normally produce no persistent resolve-cache entries, so their
new-process samples are not described as disk-cache hits.

A separate, fixed-size control resolves one real venv. Its cold process must
start exactly one instrumented interpreter and write a nonempty persistent
cache entry before a normal bounded shutdown. A new PET process then resolves
the unchanged interpreter from the same cache without starting an interpreter,
and a same-process warm resolve must do the same. All three resolved identities
must match the submitted fixture's exact executable, prefix, `Venv` kind, and
full runtime `sys.version_info` string. The expected version is queried once
from the Python used to create the copied venvs, outside measured PET
operations. The artifact labels their latencies `cold`, `diskWarm`, and
`sameProcessWarm` and records one sample and the observed probe count for each.

The benchmark also measures request-relative time to first fixture environment (excluding ambient host results),
concurrent resolve latency, and sampled process-specific resident memory,
threads, and handles or descriptors where the platform exposes them reliably.
Resource values are observed snapshot maxima, not lifetime peaks. Resident
memory is process RSS, not exact retained heap. macOS reports thread and
descriptor counts as unavailable rather than substituting zero. Cache and
resource samples are taken outside request timing. The JSON line prefixed with
`SESSION_METRICS` includes every sample, explicit pass status, and exact
measurement counts.

The fast workload sweeps 1, 10, and 100 environments with one first-process
sample, one new-process-after-first-refresh sample, and three same-process warm
samples per size:

```console
cargo test --release --features ci-perf -p pet --test session_performance long_lived_session_benchmark -- --nocapture
```

The stress workload sweeps 1, 10, 100, and 1000 environments with one
first-process sample, one new-process-after-first-refresh sample, ten
same-process warm samples per size, and ten overlapping resolves:

```console
PET_SESSION_STRESS=1 cargo test --release --features ci-perf -p pet --test session_performance long_lived_session_benchmark -- --nocapture
```

In PowerShell, set `$env:PET_SESSION_STRESS = "1"` for the stress invocation.
Set `PET_SESSION_PYTHON` to an alternate Python executable when the default
`python`/`python3` cannot create a runnable copied venv (for example,
`PET_SESSION_PYTHON=/usr/bin/python3` in WSL).
`.github/workflows/session-benchmarks.yml` runs the fast workload for pull
requests and the stress workload weekly or through its manual `stress` mode.
The workflow keeps Cargo output in a target directory inside its checkout and
uploads only the privacy-safe `session-metrics.json` artifact, never fixture
paths or raw benchmark output. A dedicated parser rejects missing, malformed,
duplicate, mode-inconsistent, count-inconsistent, or shape-inconsistent
payloads. It also recomputes the observed resource peak from every reported
snapshot and the RSS delta from the pre-resolve and final snapshots, rejecting
either derived value unless it matches exactly. Artifact writes are atomic, and
the timeout fallback replaces corrupt artifacts rather than uploading invalid
JSON. Failed runs retain validated
measurements when available; failures without usable metrics have explicit failed
status and zero counts. The workflow preserves the original benchmark failure.

Resolve overlap is established in an untimed proof pass by a fixture barrier.
Only environments below the fixture resolve root enter the barrier; resolved
real paths keep macOS temporary-directory aliases equivalent, while ambient
interpreters bypass it. During the proof pass,
every distinct interpreter process records entry before the client issues
`info`, reconfigures to a new workspace while retaining the process's original
cache directory, and refreshes that known inventory. Every cache file captured
before reconfiguration must still exist with identical bytes afterward; cache
files added for unrelated global discoveries are allowed. The barrier remains
held until those responsiveness checks complete.
Platform-global locators may also report host installations and managers. The
benchmark converts only configured workspace entries to strict fixture
identities, validates that fixture-scoped managers remain empty, and records
only counts of unrelated global discoveries and managers, never their paths.
The reported ambient maxima include every timed, churn, and overlap refresh;
artifact validation rejects overlap counts above those maxima.
Two fast or five stress pre-released batches then use fresh, distinct
interpreters for unobstructed client-latency and resource-cycling samples.
Every warm-up, overlap, and latency response is paired with its submitted
fixture and must return that same exact identity and runtime version; merely
returning non-null JSON is not sufficient.
Process creation is therefore intentional cold-resolve work; refresh timings
create no fixture process. Resource snapshots are taken after each inventory
size and resolve batch, outside request timing. The reported observed peak is
the maximum of those actual snapshots, while post-workload thread and
handle/descriptor counts must not exceed the measured post-overlap observation.
The pre-resolve snapshot remains in the metrics so lazy worker-pool growth from
the first concurrent batch is visible rather than treated as a leak or hidden.
An empty disk cache is reported as such and is not treated as evidence of a
retention bound. Every measured server must complete an explicitly successful
stdin-close shutdown before successful metrics are emitted; `Drop` cleanup is
only a failure fallback.
