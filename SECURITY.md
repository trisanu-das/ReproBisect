# Security and privacy

## Security model

ReproBisect executes the configured project build command. Build input must therefore be treated as **untrusted code execution inside the selected OCI runtime**, not as passive file analysis.

Docker and Podman provide process/filesystem isolation for experiments, but ReproBisect is **not a hardened sandbox for hostile code**. In particular, a Docker daemon or privileged container runtime is itself a high-value host capability. ReproBisect never intentionally mounts the host Docker socket into the build container and does not automatically inject host credentials.

Use a dedicated CI worker or otherwise disposable build host for projects you do not trust.

## Secrets

Do not put long-lived credentials in `[build.env]` unless the build genuinely requires them. ReproBisect redacts configured environment-variable values in persisted environment evidence, but it cannot prevent a build command from printing a secret to stdout/stderr. Captured build text is bounded, not semantically redacted.

Network/syscall traces are processed into coarse summaries; raw syscall text and exact remote endpoints are temporary. Custom package-cache paths and cache member names are not persisted in normal evidence. Source/dependency hashes and selected standardized package metadata are persisted because they are diagnostic evidence.

## Execution bounds and cancellation

Each check, diagnosis, comparison, or fix operation shares one monotonic execution context, including fix diagnosis and verification. Output discovery and doctor have their own bounded operation context. Image inspection, pulls, runtime availability, Git provenance, toolchain probes, subset trials, baseline controls, and confirmation builds are charged before dispatch; child runners inherit the context rather than constructing a fresh allowance. There is no automatic retry or resume of a cancelled or unknown-outcome build.

```toml
[execution]
max_dispatches = 4096          # default cumulative subprocess allowance
total_timeout_seconds = 3600 # default total monotonic operation deadline

[execution.resources]
restricted = true             # opt-in; compatibility default is false
cpus = 2                      # restricted default
memory_bytes = 1073741824     # restricted default (1 GiB)
pids = 256                    # restricted default
network = "none"              # restricted default
```

Zero dispatches or zero total seconds stops before launch and is not a reproducibility result. Counts above 1,000,000, deadlines above seven days, zero resource limits, unknown fields, negative/non-integer values, and unsupported network modes are rejected. Fixed CPU/network policy cannot conflict with the dimension being tested or compared. CPU, memory, PID, and network settings are held constant across arms except an explicitly selected tested dimension in the compatibility profile. The compatibility profile does not impose CPU/memory/PID limits when these fields are omitted; it is not a restricted sandbox.

Infrastructure clients have a 30-second per-call ceiling in addition to the shared deadline; build commands also retain their configured per-attempt timeout. Complete promotion groups reserve repetitions, matched controls where required, and reversal capacity atomically. Reservations are conservative: each confirmation build reserves up to eleven dispatches (build, cold-image inspection/pull/reinspection, toolchain probe, and three cleanup calls for each possible container). Cache savings and cancelled/lost work are not refunded. Failure to reserve the whole group prevents its first build, not just its last confirmation.

SIGINT stops scheduling and terminates the active runtime client. Build, discovery, and toolchain-probe containers carry an operation label and a private runtime-written CID file outside all mounted paths. Three cleanup calls are reserved before creation: inspect the acknowledged exact ID and matching operation label, remove only that ID, and verify exact-ID absence. Cleanup has a private, finite three-second safety grace after cancellation/deadline expiry; this grace cannot launch experiment work. No name-based removal, prune/kill-all, or shared-label sweep is used. Accepted creation without a CID acknowledgement remains unknown and cleanup-unverified even if no container is currently visible; it is never automatically retried. A disconnected or unresponsive runtime may prevent verified cleanup; that remains an operational failure, not a claim of successful containment. Hard process termination, kernel/runtime failures, and hostile same-account interference are not recoverable guarantees.

Interrupted operations exit **5**, including report-only CI. The frozen automation v1 envelope uses its existing `operational_error` completion reason and marks completion/coverage false. Available canonical runs and partial reports remain local, with precise `execution_completion=cancelled`, `deadline_exhausted`, `budget_exhausted`, or `attempt_timeout` notes. A separate create-only `.reprobisect/executions/<operation-id>.json` receipt records accounting, completion cause, effective resource policy, and exact cleanup verification; canonical evidence schemas and historical fixtures are unchanged. Incomplete comparisons do not claim confirmed minimality, and incomplete fix verification does not claim a verified repair.

Source tree enumeration, hashing, copying, overrides, and discovery snapshots cooperatively check cancellation/deadline between entries and 64 KiB file chunks. An in-flight host filesystem call cannot be forcibly interrupted; this is not a hard host-I/O latency guarantee. Preparation interrupted before a full source digest leaves automation source identity null; partial canonical reports use an empty source_digest explicitly identified as unobserved, never a fabricated hash.

Active interrupted build/discovery captures are retained in separate create-only `.reprobisect/attempts/` records referenced by the execution receipt. Retained text is bounded; stream fingerprints cover only bytes actually observed and include stream-completion flags. Artifact samples are bounded to 128 visited entries, 64 KiB per file and 1 MiB total, with a one-second observation grace. These are possibly changing post-interrupt observations, not completed artifacts or proof of complete capture. Explicit discovery operational interruptions propagate exit 5 without writing a success config; ordinary unsuccessful advisory builds can still fall back to static inference.

Explicit restricted profiles reject network/file syscall tracing: bounded parsed trace bytes do **not** bound temporary trace disk. The compatibility profile preserves explicitly configured same-policy tracing and its existing ptrace permissions; those privileges are not silently enabled by budgeting. Until a real temporary-storage cap exists, tracing is unavailable in the restricted profile. OCI limits and timeouts reduce operational risk, not all disk exhaustion or hostile-code risks. Use disposable workers; no blanket sandbox guarantee is made.

## Fix safety

`reprobisect fix --verify` applies generated project patches only to a temporary source copy. Existing-file patches carry SHA-256 stale-file preconditions. ReproBisect does not silently edit the user's checkout.

## Reporting vulnerabilities

When ReproBisect is hosted in a public repository, report security vulnerabilities through that repository's private security-advisory channel where available. Do not include credentials, private source, raw proprietary build logs, or other secrets in a public issue.

## Supported security line

The `1.0.0` line remains the current published/supported stable line until `v1.1.0` is released. The `develop/1.1.0` branch carries the final 1.1.0 source version under release qualification; it is not a separately supported stable security line until publication. Earlier `1.0.0-rc.*` and `0.1.0-alpha.*` snapshots are historical development artifacts.
