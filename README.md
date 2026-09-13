# ReproBisect

ReproBisect **1.0.0-rc.1** is a causal debugger for non-reproducible builds. The first stable-line candidate targets Linux containerized builds with an explicit command and declared output files, using controlled interventions rather than treating binary differences as root causes.

The core rule is stricter than ordinary binary diffing: **a difference or suspicious string is not a root cause**. ReproBisect establishes a repeatable baseline, intervenes on controlled environmental inputs, rebuilds, inspects the resulting artifact deltas, and—by default—reverts to the baseline before making a strong causal claim.

## Release-candidate status

`1.0.0-rc.1` freezes the 1.0 CLI/status vocabulary and persisted evidence schemas. Final `1.0.0` promotion requires the compiler/MSRV, Docker, Podman, seven-case real-world corpus, dependency-lock, and deterministic packaging gates in [`docs/release-qualification.md`](docs/release-qualification.md). A skipped qualification gate is not considered a pass.

The real-world corpus uses immutable Git commits and digest-pinned external OCI inputs. User project configs may still use image tags; each experiment records the runtime-resolved immutable image identity.

### Exit codes

- `0` — operation-specific success;
- `1` — completed diagnostic operation with a finding/inconclusive outcome;
- `2` — CLI usage error from Clap;
- `5` — operational/internal error.

Automation should use JSON report status in addition to the exit code.

## Current experiment loop

```text
source snapshot
   ↓
canonical baseline × N
   ↓ stable?
   ├─ no  → UNCONTROLLED_NONDETERMINISM
   └─ yes
        ↓
one-variable interventions
        ↓
SHA-256 + typed binary/package/archive evidence
        ↓
reversion confirmation
        ↓
optional interaction search / ddmin over individually inert variables
        ↓
evidence-backed diagnoses
        ↓
optional rule-based fix candidate + re-run of original failing intervention
```

Implemented intervention dimensions:

- coarse **build/container image** variants with resolved image/toolchain provenance,
- narrow **toolchain executable bindings** (`CC`, `CXX`, `LD`, `AR`, `RANLIB`, `RUSTC`) within the same image,
- one-file **dependency declaration/lockfile replacements** in the fresh workspace,
- **network availability** (`default` vs `--network none`),
- independent **source path** and **build/workspace path**,
- `SOURCE_DATE_EPOCH`,
- source-tree filesystem mtimes,
- timezone (`TZ`),
- locale (`LANG` / `LC_ALL`),
- container hostname,
- user-declared environment variables,
- CPU quota/count with interleaved matched stochastic trials and one-sided Fisher exact evidence,
- process umask,
- best-effort source materialization/directory-order perturbation.

Implemented evidence includes:

- streaming SHA-256 for declared outputs,
- exact controlled-value marker scans,
- ELF section-table parsing with controlled-value localization to specific `.debug_*` sections,
- GNU ELF build-ID extraction,
- TAR, ZIP, `ar`, and gzip metadata inspection,
- structural refinement of ZIP/ar/TAR outputs into JAR, Python wheel, DEB, and OCI-image tar types, plus bounded PE/COFF, Mach-O, and WebAssembly header detection,
- bounded non-executing semantic summaries for selected JAR manifests, wheel `.dist-info` metadata, DEB container structure, OCI layout/index JSON, PE/COFF headers, Mach-O headers/UUIDs, and Wasm custom-section structure,
- archive member order, mtime, uid/gid, mode, size, and container mtime deltas,
- source-control and dependency-lock fingerprints, privacy-preserving parsed dependency-resolution summaries, controlled toolchain-binding probes with observed invocation counts, and best-effort image/toolchain provenance,
- optional runtime dependency-cache provenance using before/after aggregate content fingerprints without persisting cache paths or member names,
- optional redacted network-syscall summaries with call/success/failure counts plus endpoint-scope classes (`public`, `private`, `loopback`, `unix`, etc.); exact endpoints are not persisted.

## Quick start

Create a starter config:

```bash
reprobisect init .
```

Minimal `.reprobisect.toml`:

```toml
[build]
image = "gcc:14"
command = ["sh", "-lc", "make"]
outputs = ["build/app"]
timeout_seconds = 600
log_capture_max_bytes = 1048576

[experiments]
control_runs = 2
intervention_runs = 1
confirmation_runs = 1
stochastic_runs = 4
stochastic_alpha = 0.05
image_variants = []
network_trace = false
file_input_trace = false
syscall_trace_max_bytes = 33554432
runtime_dependency_provenance = false
dependency_cache_max_files = 2048
dependency_cache_max_bytes = 134217728

# Optional additional package-manager cache roots inside the container:
# [experiments.dependency_cache_paths]
# custom = "/root/.cache/custom"

[experiments.dimensions]
network_access = false
source_path = false
build_path = true
source_date_epoch = true
timezone = true
locale = true
hostname = true
source_mtime = false
cpu_count = false
umask = false
directory_order = false
```

### Bounded build logs and experiment budgets

ReproBisect drains build stdout/stderr concurrently so a verbose build cannot deadlock on a full pipe. Persisted text is bounded per stream by `build.log_capture_max_bytes` (default **1 MiB**, accepted range **64 KiB–16 MiB**), while the complete raw stream is still counted and SHA-256 hashed. `BuildRun` / `BuildFailure` evidence records the configured cap, total byte count, full-stream digest, and whether persisted text was truncated.

This distinction matters for known-bad failure comparison: failure identity continues to use the complete stdout/stderr SHA-256 values even when only a prefix is retained for human-readable evidence.

Ordinary deterministic repetition budgets are also bounded. Control, intervention, interaction, and environment-comparison run counts are capped at **32** per configured repetition dimension; command-line overrides are subject to the same ceilings. Stochastic CPU probing remains separately bounded at **3–1024** matched pairs because its sample size is part of the statistical test.

Run the causal check:

```bash
reprobisect check .
```

`diagnose` is an explicit alias for the same pipeline:

```bash
reprobisect diagnose .
```

For machine-readable evidence:

```bash
reprobisect check . --format json
```

## Toolchain/image and network hermeticity experiments

A project may opt into coarse build-image comparisons:

```toml
[experiments]
image_variants = ["gcc:15"]
```

Every configured image reference is resolved to an immutable image ID through the selected OCI runtime before execution. ReproBisect also records best-effort compiler/linker/tool versions from the image. A positive image experiment is capped below high confidence because changing an image can change many inputs simultaneously; it is evidence that the **build environment image matters**, not proof that one compiler caused the result.

Network availability is an independent opt-in dimension:

```toml
[experiments.dimensions]
network_access = true
```

The variant executes with Docker `--network none`. A repeatable non-zero build-command exit is retained as evidence that the tested build flow requires network availability. Runner/Docker failures remain infrastructure errors and are not converted into causal evidence. This experiment does not yet identify endpoints or downloaded bytes.

ReproBisect also records the containing Git commit/dirty state, hashes common dependency lock/manifest files, resolves image IDs, and records best-effort toolchain versions. Parsed resolution summaries now cover Cargo, npm/pnpm/yarn, Poetry/Pipenv/uv/pinned requirements, Go, Bundler, Composer, and Gradle. Persisted parsed provenance contains only ecosystem, package count, and a hash of normalized coordinates; package names, registry URLs, and raw lockfile contents are not copied into that parsed provenance record. These are provenance records, not a claim that the build is fully hermetic.

For a narrower toolchain experiment within one image:

```toml
[experiments.toolchain_variables]
CC = ["gcc", "clang"]
```

The selected binding is exposed through a stable wrapper path inside the same resolved image. The wrapper delegates to the controlled executable and records only an invocation count—never command-line arguments. A toolchain diagnosis reaches high confidence only when both baseline and variant executables were actually observed being invoked, the artifact effect is repeatable, and baseline reversion succeeds. This makes `AR`/`LD`/`CC` experiments materially narrower than image-level comparisons without pretending that an uninvoked environment binding caused anything.

A dependency declaration can be varied one file at a time without touching the checkout:

```toml
[[experiments.dependency_variants]]
id = "candidate-lockfile"
target = "requirements.txt"
variant_file = "requirements.variant.txt"
```

ReproBisect copies the project into a fresh workspace and replaces only `target` there. Evidence records the target and before/after SHA-256 values; it does not persist parsed private package coordinates from the variant file.

Optional best-effort syscall provenance is opt-in. `network_trace = true` records redacted network/process classes; `file_input_trace = true` records process-aware dependency/cache opens, readable mappings of classified descriptors, declared-output writes, and rename publication into declared outputs. When either mode is enabled, ReproBisect grants `SYS_PTRACE` only to that build container and performs an in-container `strace` preflight. If tracing is unavailable or blocked, the real build runs untraced and provenance is marked unavailable instead of turning instrumentation failure into a project build failure. Raw traces remain ephemeral under `/reprobisect-meta`. Persisted evidence contains only aggregate syscall classes, coarse endpoint scopes, anonymous process identifiers and parent links, coarse process roles, lineage depth, and event counts. Exact IP addresses, ports, hostnames, argv, container PIDs, dependency/cache paths, temporary paths, and raw traces are not persisted.

Trace parsing is bounded by `syscall_trace_max_bytes` (default 32 MiB; configurable from 1 to 512 MiB). Truncation is explicit. The bound limits parser memory/input, not temporary `strace` disk growth. Phase 11 additionally follows dependency/cache descriptor identity across `dup*`, inherited descriptors across traced process creation, readable `mmap`, and temporary-file rename chains long enough to report same-lineage publication into a declared output. It can therefore report both same-process and strict-ancestor input/network/output co-occurrence while keeping raw PIDs and paths ephemeral. These correlations are still **not byte-level taint/dataflow proof**: IPC/shared memory, `close_range`, platform-specific mapping syscalls, unresolved `dirfd`/cwd path semantics, and arbitrary in-memory propagation remain outside the model.

Runtime package/dependency provenance is independently opt-in:

```toml
[experiments]
runtime_dependency_provenance = true
dependency_cache_max_files = 2048
dependency_cache_max_bytes = 134217728

[experiments.dependency_cache_paths]
my-cache = "/tmp/my-package-cache"
```

For known package-manager cache roots plus any configured roots, ReproBisect records bounded before/after content summaries. Per cache root it hashes at most `dependency_cache_max_files` files and at most `dependency_cache_max_bytes` cumulative bytes. A summary records file count, sampled byte count, aggregate SHA-256, and whether the bound truncated observation. If either side is truncated, the comparison is explicitly incomplete: ReproBisect may report an observed difference, but it does **not** promote that difference to a confirmed cache mutation. Directory traversal itself remains best-effort; the bounds limit content hashing work rather than proving constant-time cache enumeration.

Cache paths, member names, package coordinates, and raw package-manager output are not persisted. Configured custom cache-root mappings are runtime-only and are explicitly omitted from serialized controlled-environment evidence. When `network_trace = true` is enabled at the same time, ReproBisect records coarse build-window co-occurrence between successful non-local network activity and complete cache mutations. With `file_input_trace = true`, it can additionally report whether the same anonymous process read a dependency declaration or dependency cache and wrote a declared output; if network tracing is also enabled it can report same-process network/cache/output co-occurrence. These signals are **not** byte provenance: they do not identify which response or input byte influenced a particular output byte.

## Independent source and build paths

The controlled baseline exposes the fresh source snapshot at two container aliases:

```text
source path: /src
build path:  /workspace
```

Ordinary in-source commands can ignore `/src` and continue to execute in `/workspace`. To make an out-of-tree or explicit-source command participate in an independent source-path experiment, command arguments may contain `{source}` and `{build}` placeholders:

```toml
[build]
image = "gcc:14"
command = [
  "sh", "-lc",
  "mkdir -p {build}/build && gcc -g -o {build}/build/app {source}/main.c"
]
outputs = ["build/app"]

[experiments.dimensions]
source_path = true
build_path = false
```

ReproBisect substitutes the controlled container paths before invoking Docker. The source-path intervention changes only the source alias; the build-path intervention changes only the cwd/build alias.

## Project-specific environment variables

ReproBisect cannot safely guess which arbitrary environment variables are intended build inputs. Declare the variables to perturb:

```toml
[experiments.environment_variables]
BUILD_FLAVOR = ["alpha", "beta"]
FEATURE_SET = ["baseline", "variant"]
```

The first value becomes part of the canonical baseline and the second is the intervention value.

## Multi-variable interactions

Single-variable experiments cannot detect a build that changes only when two or more inputs move together. Interaction search is therefore available explicitly:

```toml
[experiments]
interaction_search = true
interaction_runs = 2
max_interaction_variables = 8
```

ReproBisect considers **individually inert, successfully tested variables**, first checks their combined effect, then uses a ddmin-style search to find a 1-minimal observed failure-inducing set. The minimized set is repeated and baseline-reverted before it is promoted to an interaction diagnosis.

Interaction results are scoped to the tested candidate set; they are not claimed to be globally unique root causes.

## Stochastic parallelism

Enable CPU/parallelism experiments explicitly:

```toml
[experiments]
stochastic_runs = 8
stochastic_alpha = 0.05

[experiments.dimensions]
cpu_count = true
```

The canonical baseline uses one CPU and exports `REPROBISECT_CPU_COUNT=1`; the intervention uses two CPUs. For each stochastic trial ReproBisect now runs an interleaved baseline reference followed by one variant build. Each run is classified by whether its declared-artifact signature differs from the original stable baseline signature. A fixed-sample, one-sided Fisher exact test compares the baseline-reference and variant change rates. The persisted evidence includes both rates, their absolute difference, the p-value, alpha, and a conservative classification.

A statistical result is promoted only when the variant change rate is larger and `p <= stochastic_alpha`. If any matched baseline reference itself changes, the earlier control-stability assumption has been contradicted: the overall check becomes `UNCONTROLLED_NONDETERMINISM`, causal diagnoses are suppressed, and interaction ddmin is skipped. The test quantifies a distribution shift under the tested CPU setting; it does not localize the internal race or correct automatically for an arbitrary family of many stochastic hypotheses.

## `reprobisect fix`

The fix pipeline is deliberately rule-based. For supported medium/high-confidence diagnoses, ReproBisect can propose operational normalizations without editing project files:

```bash
reprobisect fix .
```

To experimentally verify a supported candidate:

```bash
reprobisect fix . --verify
```

Current candidates cover compiler path remapping, `SOURCE_DATE_EPOCH` pinning, source-mtime normalization when archive metadata proves mtime sensitivity, and umask/mode normalization when archive metadata proves mode sensitivity. GNU-tar source-mtime/umask candidates now have conservative project patch points for conventional Make builds, one unambiguous literal tar invocation in CMake or Meson, and one detected packaging shell script. CMake uses `${CMAKE_COMMAND} -E env`; Meson and shell-script candidates scope `TAR_OPTIONS` around the selected command. Ambiguous multi-site packaging logic is refused. Every existing-file replacement carries a SHA-256 stale-file precondition and is verified only in a temporary checkout. When no conservative tar patch point exists, they fall back to runner-level operational normalization. Path-remapping patches now have conservative build-system-specific insertion points: root `Makefile`, root `CMakeLists.txt`, root `meson.build`, or a newly created `.cargo/config.toml` when no Cargo config already exists. `SOURCE_DATE_EPOCH` can likewise use a temporary Cargo config for Cargo builds or the existing Makefile rule. Existing-file patches carry a SHA-256 stale-file precondition; create patches refuse to overwrite an existing path. Verification copies the project to a temporary tree, applies only that patch, and replays the original failing intervention. The user's checkout is never modified. If no project patch is available, compiler remapping falls back to an existing declared compiler-flag environment injection point.

A candidate is marked `VERIFIED` only when:

1. repeated fixed-baseline builds are byte-stable,
2. repeated builds under the **original failing intervention** are byte-stable, and
3. the intervention artifacts are byte-identical to the fixed baseline.

No source, Makefile, or build script in the user checkout is modified automatically. Project patch candidates are applied only to temporary verification copies.
Persisted fix plans contain only the proposed flag additions; existing compiler-flag values are combined with them in memory during verification and are not copied into fix evidence.

## Status semantics

`REPRODUCIBLE_WITHIN_TESTED_SPACE` means the declared outputs were byte-identical across the repeated controlled baseline and every successfully completed configured experiment. It is not proof of universal reproducibility.

`NON_REPRODUCIBLE` means a stable baseline produced different declared artifact bytes under at least one explicit intervention or confirmed interaction.

A repeatable build-command failure with networking disabled produces a network-dependence diagnosis but leaves the overall byte-reproducibility result `INCONCLUSIVE`, because no comparable offline artifact exists.

`UNCONTROLLED_NONDETERMINISM` means repeated builds under the same controlled baseline differed. Deterministic attribution stops at that gate.

`INCONCLUSIVE` means the baseline was stable but one or more requested experiments could not be completed, so ReproBisect refuses to make a positive reproducibility claim.

## Evidence and privacy

Evidence is stored locally under `.reprobisect/` and excluded from source hashing/workspace copies:

```text
.reprobisect/runs/<experiment-id>/run-....json
.reprobisect/experiments/<experiment-id>.json
.reprobisect/fixes/<diagnosis-experiment-id>.json
.reprobisect/comparisons/<experiment-id>.json
```

Evidence files are create-only. Values from ordinary `[build.env]` entries are passed to the selected OCI runner but redacted in persisted run manifests. Explicit experiment values remain visible because they form part of the causal record.

### Historical evidence compatibility

Phase 19 adds an explicit reader/migration boundary for persisted JSON evidence:

```bash
reprobisect evidence .reprobisect/experiments/<id>.json
reprobisect evidence old.json --normalized-output normalized.json
```

`--format json` prints the normalized current-schema document. `--normalized-output` is create-only unless `--force` is passed. Even with `--force`, the source evidence path cannot be used as the destination and an existing destination symlink is refused. Input JSON is bounded to **64 MiB**.

The supported historical boundary is the frozen **Phase 6** release. Current Phase 19 readers accept CheckReport schemas **5–14**, BuildRun/BuildFailure schemas **1–10**, FixReport schemas **2–12**, and EnvironmentComparisonReport schemas **1–4** (comparison evidence begins in Phase 14). Older schemas are rejected rather than heuristically guessed, and future schema versions are rejected with an explicit upgrade error.

Normalization fills only compatibility-safe defaults and performs two explicit historical migrations. Pre-Phase-18 run/failure records stored complete UTF-8 stdout/stderr text but no raw-stream digest metadata; normalization derives the same text-based hashes and byte counts used by the older failure-identity logic. This cannot reconstruct original non-UTF8 raw bytes. Phase-8 unbounded dependency-cache summaries are marked complete/non-truncated and use `max_files = 0`, `max_bytes = 0` as **unknown historical bounds** rather than inventing a limit. The compatibility command reports both migrations.

The frozen Phase 6–18 schema sequence is checked by `tests/evidence-schema-matrix.json`; current schema constants are centralized in `src/schema.rs` so producers and readers cannot drift through copied numeric literals. See [`docs/evidence-compatibility.md`](docs/evidence-compatibility.md) for the compatibility contract.

Marker scanning is bounded and searches only controlled experiment values; ReproBisect does not persist unrestricted binary string dumps.

## Development

```bash
python3 scripts/static-check.py
cargo check --all-targets
cargo test --all-targets
./scripts/test-fixtures.sh   # requires Docker
python3 scripts/real-world-corpus.py validate
python3 scripts/real-world-corpus.py run --tier smoke --binary target/debug/reprobisect
```

The primary Docker fixture suite covers positive and negative causal cases, structural archive/package metadata and type-aware JAR/wheel/DEB/OCI/PE/Mach-O/Wasm classification, stochastic parallelism, a two-variable interaction, independent source-path sensitivity, build-image/network hermeticity cases, observed `CC`/`AR` invocation, bounded runtime dependency-cache provenance, multi-ecosystem normalized lockfile provenance, process-aware dependency/cache/output provenance, and verified Make/Cargo/shell path/timestamp/mtime/umask normalization rules including project-level GNU tar fixes.

Phase 20 also includes a separate **pinned real-world OSS corpus**. It fetches exact upstream commits instead of vendoring source and keeps the overlay invisible to Git dirty-state logic. Normal CI runs a four-case Docker smoke tier; an explicit workflow runs the heavier extended tier with Docker or Podman. Aggregate corpus JSON records the exact ReproBisect binary SHA-256 and full per-case reports. See [`docs/real-world-validation.md`](docs/real-world-validation.md) and [`corpus/README.md`](corpus/README.md).

See [`docs/problem-statement-and-v0.1-spec.md`](docs/problem-statement-and-v0.1-spec.md) for the design target and [`docs/implementation-status.md`](docs/implementation-status.md) for the precise current boundary.


## Known-good / known-bad environment comparison

When two environments are already known to produce different successful artifacts—or when a known-good build succeeds and a known-bad environment fails reproducibly—use `compare` instead of asking the generic intervention planner to rediscover the entire delta:

```bash
reprobisect compare . --good good.toml --bad bad.toml
```

A comparison manifest is intentionally narrower than `.reprobisect.toml`. It can control image, in-container source/build paths, source copy order, `SOURCE_DATE_EPOCH`, source mtimes, timezone/locale, hostname, CPU count, umask, network availability, selected environment variables, narrow toolchain bindings, and project-relative dependency-file substitutions.

Example:

```toml
# good.toml
build_path = "/workspace/good"
source_date_epoch = 1700000000
umask = "022"

[variables]
BUILD_FLAVOR = "stable"

[toolchain]
CC = "gcc"
```

```toml
# bad.toml
build_path = "/workspace/bad"
source_date_epoch = 1700001234
umask = "077"

[variables]
BUILD_FLAVOR = "candidate"

[toolchain]
CC = "clang"
```

ReproBisect first repeats both endpoints. The known-good endpoint must stably succeed and produce one declared-artifact signature. The known-bad endpoint may either stably produce a different artifact signature or stably exit non-zero. For artifact-producing bad endpoints, a subset counts as reproducing only when every repeated subset run matches the **exact known-bad declared-artifact signature**. For build-failure bad endpoints, every repeated subset run must reproduce the exact failure signature: **exit code plus SHA-256 of stdout and stderr**. Merely producing something different from known-good, or merely returning the same generic non-zero code with different diagnostics, is insufficient. The result is reported as a 1-minimal observed bad-environment delta under the tested values, search path, and repetition policy.

`[experiments] comparison_runs` and `comparison_subset_runs` default to `2` and can be overridden with `--runs` / `--subset-runs`.

Comparison-manifest controlled values are evidence and are persisted in run manifests and delta records. **Do not put credentials or secrets in comparison manifests.** Secrets that are not experimental variables belong in ordinary `[build.env]`, whose values remain redacted from persisted evidence.

Known-bad build failures are minimized conservatively. ReproBisect requires repeated bad-endpoint failures to have the same exit code and exact stdout/stderr hashes before ddmin starts. This reproduces an observed process outcome; it does not prove that two matching diagnostics came from the same semantic root cause. A known-good endpoint that itself exits non-zero remains unsupported.

## OCI runner backends

ReproBisect can execute the same controlled experiment model through either Docker or Podman:

```toml
[build]
runner = "docker" # default
# runner = "podman"
```

The selected backend is recorded in every `BuildRun` and `BuildFailure`. Image resolution, toolchain probing, build execution, timeout termination, and fix verification all use the same configured runtime. ReproBisect does not compare Docker and Podman as a causal variable in this release; changing the runner is an explicit project configuration choice.
