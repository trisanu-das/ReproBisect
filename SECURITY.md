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

## Local derived summaries and approved export

Canonical reports and build evidence remain authoritative, local and unchanged. Derived views are not canonical evidence, an anonymization guarantee, a verified repair, a source-minimized reproducer or an assertion of global reproducibility. `summary` and `export` are deterministic offline operations: they need no OCI runtime, account, model, credentials or network connector, and never discover/copy raw files, source trees, binaries or archives. Default `check`/`diagnose` behavior still performs only its previously configured build operations; it does not upload anything or create a share bundle.

```sh
reprobisect summary report.json --format json
reprobisect summary report.json --format markdown
reprobisect export report.json --output new-local-bundle
# Inspect all exact member bytes/hashes, selections and destination in that preview.
# Re-run the identical input/options/destination with the preview's approval_sha256:
reprobisect export report.json --output new-local-bundle --approve <approval_sha256>
reprobisect check project --ci --summary-output new-local-summary.json
reprobisect diagnose project --format json --summary-output new-local-summary.json
```

Inputs can be supported historical/current check reports, build runs, build failures, fix reports or environment-comparison reports. The unchanged compatibility reader parses a private bounded snapshot of the captured input. `original_input_sha256` identifies those **raw original bytes**, including whitespace and omitted data; it is not a normalized/derived digest. Artifact `reported_sha256` values are validated digest strings reported by the input, not new verification of external artifact files. Summary member hashes separately identify the exact rendered UTF-8 bytes.

The default typed allowlist contains version/kind/digest provenance, persisted diagnostic/comparison statuses, bounded observations/counts and execution/coverage state. It omits raw stdout/stderr, ordinary environment values, commands, source/binary bytes, free-form diagnostics, controlled values, artifact paths and package metadata. Source identity, experiment IDs, hostnames, report location and other private paths are not copied into shared payloads. This does **not** sanitize the pre-existing local verbose reports or all raw operational stderr; those remain local diagnostics, not shareable summaries.

Only three explicit, repeatable selectors are supported for `summary` and `export`:

- `--allow-field controlled-values`: exact experimental variable/baseline/variant strings; these can contain credentials or private paths.
- `--allow-field artifact-paths`: exact reported artifact logical paths; these can identify a private project or directory. Other private paths remain omitted.
- `--allow-field package-metadata`: only the standardized `wheel_version`, `metadata_version`, `debian_binary_version`, `image_layout_version` and `index_schema_version` attributes, not arbitrary package fields/names/diagnostics.

Selections are sorted/deduplicated and listed in the view and preview; unsupported selectors and arbitrary JSON pointers are rejected. Explicit selection is not a secret detector. Inspect selected values before sharing: bounded content, hashes, sizes, statuses and even unselected structural information can still fingerprint a private build. JSON encodings retain exact selected values while escaping HTML-sensitive characters, terminal/line/bidi controls. Markdown uses an escaped, inert preformatted view rather than active data-generated links/HTML/Actions/Azure workflow commands. Downstream consumers must not decode values into shell/workflow source or active markup.

Standalone evidence cannot reconstruct the current configured plan, so planned coverage and successful completion are `unknown`/null. Recognized P02 `execution_completion=` notes mark execution and coverage false with the precise reason (`budget_exhausted`, `deadline_exhausted`, `cancelled`, `attempt_timeout`, `cleanup_failed`, `creation_unknown` or `operational_error`); unrecognized free-form notes are not disclosed or interpreted. Persisted diagnostic truth is retained even when execution is incomplete. Fix counts are **reported** candidates/verified repairs, not independent promotion of a repair.

Opt-in `--summary-output` is a create-only bounded local JSON sidecar for `check`/`diagnose`, using the same default view. Its authoritative coverage is copied from the existing current-run CI calculation, including planned/attempted/completed/skipped/failed counts and interaction-search scope, never guessed from returned runs. Partial budget/deadline/signal outcomes retain false completion/coverage and their precise execution cause. The default ordinary/automation stdout formats, diagnostic/CI policy and exits are unchanged; no second JSON summary is printed on stdout. A sidecar failure is operational exit **5**; CI can emit the existing generic operational-error envelope without inventing a diagnostic status. Failure before a persisted report cannot fabricate a summary. Sidecars have no optional disclosure selectors and grant no unattended upload consent.

Without `--approve`, export emits a complete bounded local preview and creates no destination/bundle. The preview contains all three fixed member names (`summary.json`, `summary.md`, `manifest.json`), exact UTF-8 member bytes, sizes and SHA-256 hashes, raw input digest, policy/renderer versions, allowlist and escaped canonical local destination. The approval SHA-256 binds raw input identity, all exact member bytes through their digests/sizes/names, selection, policy/renderer and destination. Everything is recomputed before a create-only write. Wrong/stale approval, changed input bytes (even omitted data/whitespace), changed selections or destination fail with exit **5**, before bundle writes. Identical captured bytes remain the same content identity regardless of the input filename. This is a public content binding, **not authentication**, a signed attestation or standing permission to upload future evidence. The shared manifest has no local destination/input-directory paths; only the local preview reveals the destination.

Limits are 4 MiB captured input, 1 KiB per selected string, 128 retained runs/artifacts/interventions/failure observations per bounded class, 128 KiB per rendered member/sidecar, 192 KiB aggregate exported members, and 1 MiB for the escaped preview wrapper. Oversized/malformed inputs, missing parents and unsafe paths fail closed rather than truncate a purported complete member. Paths are UTF-8, at most 4096 OS-string units with 255-byte normal components. No parent traversal, source/destination symlink, linked ancestor, Windows reparse point, drive-relative/UNC/device/verbatim prefix, Windows reserved device name, trailing dot/space, alternate data stream or unsafe component is accepted. The source is inspected as a regular file before opening; the captured open file is also checked. Destination parents must already exist; neither bundles nor sidecars replace existing targets. Member names never come from untrusted input.

Path inspection and create-only writes protect cooperative local use, **not race-proof isolation from concurrent same-account filesystem mutation**. Use a private trusted parent directory; hostile same-account link swapping, mount replacement or mutation during read can defeat path assumptions. Hashing and parsing use the same captured bounded bytes, not a second source read, but those captured bytes need not reflect a stable point-in-time file under interference. Filesystem calls themselves can block. Approved writes can leave a partial new directory/file after disk or transport failure; inspect or remove it manually before retrying with a new destination. There is no destructive automatic rollback, source overwrite, implicit raw-log upload or network publication. Sending any derived bundle elsewhere is a separate action with separate destination-specific review/consent; P03 provides no connector, static upload flag or model integration.

## Fix safety

`reprobisect fix --verify` applies generated project patches only to a temporary source copy. Existing-file patches carry SHA-256 stale-file preconditions. ReproBisect does not silently edit the user's checkout.

## Reporting vulnerabilities

When ReproBisect is hosted in a public repository, report security vulnerabilities through that repository's private security-advisory channel where available. Do not include credentials, private source, raw proprietary build logs, or other secrets in a public issue.

## Supported security line

The `1.0.0` line remains the current published/supported stable line until `v1.1.0` is released. The `develop/1.1.0` branch carries the final 1.1.0 source version under release qualification; it is not a separately supported stable security line until publication. Earlier `1.0.0-rc.*` and `0.1.0-alpha.*` snapshots are historical development artifacts.
