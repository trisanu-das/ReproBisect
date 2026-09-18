# ReproBisect 

ReproBisect is a *causal debugger* for non-reproducible builds.
When the same source produces different artifacts on different machines, paths, timestamps, toolchains, or environments, tools such as binary diffing can tell you **what changed**. ReproBisect tries to determine **what caused it**. It repeatedly rebuilds your project in controlled containers, changes one environmental input at a time, and confirms a diagnosis by reverting to the baseline.

```text
same source
   │
   ├── baseline build
   ├── baseline build          ← first establish stability
   │
   ├── change build path only
   ├── change timezone only
   ├── change locale only
   ├── change SOURCE_DATE_EPOCH only
   │
   └── revert to baseline      ← confirm the effect disappears
```

If changing only the build path repeatedly changes the artifact, and reverting the path restores the original result, ReproBisect has evidence that the **build path is causal** rather than merely present somewhere in the binary.

## Usage Example

Suppose this succeeds:

```bash
make
```

but `build/app` differs depending on where the project is built.

Create `.reprobisect.toml`:

```toml
[build]
runner = "docker"
image = "gcc:14"
command = ["make"]
outputs = ["build/app"]
```

Then run:

```bash
reprobisect check .
```

ReproBisect will:

1. build the project repeatedly to verify the baseline is stable.
2. rebuild it under controlled environmental changes.
3. compare the declared artifact.
4. localize relevant differences where possible.
5. revert successful interventions before making strong causal claims.

For machine-readable evidence:

```bash
reprobisect check . --format json
```

The 1.1 text report is diagnosis-first: the default view leads with result, promoted cause and confidence, affected artifacts, baseline/variant/reversion hashes when available, evidence, remediation, and a compact effect/no-effect list of tested variables. Use `-v` when you need the full run, provenance, trace, and build-log detail.

---

## Use-Case

Use ReproBisect when you have a question like:

> “Which environmental input is responsible for two differing builds?”

Typical cases might be:

* debug paths embedded in ELF binaries.
* timestamps or filesystem mtimes leaking into artifacts.
* archive member metadata changing.
* locale, timezone, hostname, or umask affecting output.
* compiler/linker/archive-tool differences.
* dependency declaration or lockfile changes.
* build-image differences.
* network-dependent builds.
* parallelism-sensitive or stochastic build behavior.
* interactions between multiple otherwise harmless variables.

### A comparision between existing tools and ReproBisect

| Tool / approach     | Best question                                                                                                  |
| ------------------- | -------------------------------------------------------------------------------------------------------------- |
| **diffoscope**      | *What bytes, sections, files, or metadata differ?*                                                             |
| **reprotest**       | *Does this build remain reproducible under environment variation?*                                             |
| **manual rebuilds** | *Does changing this thing seem to affect my build?*                                                            |
| **ReproBisect**     | *Which tested environmental change causally explains the artifact difference, with repeat/reversion evidence?* |

A example of useful workflow using a combination of the above could be:

```text
ReproBisect → identify causal dimension
diffoscope → deeply inspect the resulting artifact difference
```

NOTE: ReproBisect is not intended to replace a detailed binary diff.

---

## Installation Guide

### Prebuilt Linux release

Download the latest Linux archive from the GitHub Releases page and extract it:

```bash
tar -xzf reprobisect-1.0.0-x86_64-unknown-linux-gnu.tar.gz
```

Install the binary somewhere on your `PATH`:

```bash
sudo install -m 0755 \
  reprobisect-1.0.0-x86_64-unknown-linux-gnu/reprobisect \
  /usr/local/bin/reprobisect
```

Verify:

```bash
reprobisect --version
```

Expected:

```text
reprobisect 1.0.0
```

### Build from source

ReproBisect declares Rust 1.85 as its MSRV; the repository pins and qualifies Rust 1.85.1 for development and release testing.

The public stable release remains **1.0.0** until a `v1.1.0` tag/release is created. The `develop/1.1.0` branch now carries the final **1.1.0** source version for qualification, so building that branch from source reports `reprobisect 1.1.0`; the prebuilt stable instructions above intentionally continue to reference 1.0.0 until the release is published.

```bash
git clone https://github.com/trisanu-das/ReproBisect.git
cd ReproBisect
cargo build --locked --release
```

Then:

```bash
./target/release/reprobisect --version
```

### Requirements

* You need a working OCI container runtime(Docker/Podman).
* ReproBisect currently targets Linux containerized builds.

---

## Quick start

Generate a starter configuration:

```bash
reprobisect init .
```

On the 1.1 release branch, `init` deterministically inspects common build-system markers and proposes an editable image, build command, and likely output artifact. It recognizes Cargo, Go, npm/pnpm, Maven, Gradle, Bazel, Meson, CMake, Autotools, Python packaging, and Make projects. The command reports its confidence and any ambiguous markers; low-confidence output guesses are called out explicitly rather than treated as authoritative. Review the generated `.reprobisect.toml` before the first build.

When static output inference is ambiguous, you can opt into a single temporary containerized build and let ReproBisect rank the files that build created or changed:

```bash
reprobisect init . --discover-outputs
```

The probe snapshots a temporary workspace before and after the detected build, filters common intermediates, and classifies executable/archive/package candidates. The full ranked shortlist is printed, but only **high-confidence** candidates are allowed to replace the statically inferred `build.outputs`; medium/low-confidence candidates remain advisory. It never builds in or mutates your checkout. Probe log retention is bounded and the temporary build is stopped after 600 seconds. Docker is the default runtime; use `--runner podman` to generate and probe with Podman instead.

Or create one yourself:

```toml
[build]
runner = "docker"
image = "gcc:14"
command = ["make"]
outputs = ["build/app"]
```

Before spending time on controlled rebuilds, check that the project configuration and OCI runtime are ready:

```bash
reprobisect doctor .
```

`doctor` validates the configuration, checks that the configured Docker/Podman executable is available, verifies that the runtime is reachable, and reports whether the configured build image is already present locally. A missing local image is a warning rather than a failure because the runtime can normally pull it when the build starts.

For machine-readable readiness information:

```bash
reprobisect doctor . --format json
```

Then run the causal experiment:

```bash
reprobisect check .
```

`diagnose` is an explicit alias:

```bash
reprobisect diagnose .
```

### Using Podman

```toml
[build]
runner = "podman"
image = "gcc:14"
command = ["make"]
outputs = ["build/app"]
```

The rest of the workflow is unchanged.

---

## What does ReproBisect vary?

The default deterministic experiment set covers several common sources of build nondeterminism, including:

* build/workspace path
* `SOURCE_DATE_EPOCH`
* timezone
* locale
* hostname

Additional dimensions can be enabled explicitly.

For example:

```toml
[experiments.dimensions]
network_access = true
source_path = true
source_mtime = true
cpu_count = true
umask = true
directory_order = true
```

ReproBisect also supports more targeted experiments such as alternate build images, specific toolchain executables, dependency files, and user-declared environment variables.

### Toolchain experiment

Instead of swapping an entire image, compare a single tool within the same image:

```toml
[experiments.toolchain_variables]
CC = ["gcc", "clang"]
```

Supported bindings include tools such as:

```text
CC
CXX
LD
AR
RANLIB
RUSTC
```

ReproBisect records whether the selected executable was actually invoked before treating the result as strong evidence.

### Dependency-file experiment

Test one dependency declaration or lockfile replacement:

```toml
[[experiments.dependency_variants]]
id = "candidate-lockfile"
target = "requirements.txt"
variant_file = "requirements.variant.txt"
```

The user's checkout is not modified. The replacement happens in a fresh experiment workspace.

### Project-specific environment variable

```toml
[experiments.environment_variables]
BUILD_FLAVOR = ["release", "debug"]
```

The first value becomes the controlled baseline and the second the intervention.

---

## How causal diagnosis works

A normal deterministic experiment follows roughly this sequence:

```text
1. Snapshot source
        ↓
2. Build canonical baseline multiple times
        ↓
   stable?
   ├── no  → uncontrolled nondeterminism
   └── yes
        ↓
3. Change one controlled input
        ↓
4. Rebuild and compare artifacts
        ↓
5. Repeat the intervention
        ↓
6. Revert to baseline(IMPORTANT)
        ↓
7. Promote evidence-backed diagnoses
```

Seeing `/tmp/build-A` in one binary and `/tmp/build-B` in another is useful evidence, but it does not by itself prove that the build directory caused the difference.
ReproBisect deliberately distinguishes **correlation** from **intervention-backed diagnosis**.

---

## Artifact evidence

ReproBisect hashes every declared output with SHA-256 and can extract additional structural evidence from common artifact formats.

Current analysis includes support for evidence from:

* ELF;
* TAR;
* ZIP;
* `ar`;
* gzip;
* JAR;
* Python wheels;
* DEB packages;
* OCI image tar layouts;
* PE/COFF;
* Mach-O;
* WebAssembly.

Depending on the format, ReproBisect can inspect information such as:

* ELF sections and debug-path localization;
* GNU build IDs;
* archive member order;
* timestamps;
* uid/gid;
* permission modes;
* member sizes;
* selected package metadata;
* selected executable/container structural metadata.

The artifact analyzer is intentionally bounded and non-executing.
For deep byte-level inspection, use a dedicated tool such as `diffoscope` after ReproBisect has narrowed the causal dimension.

---

## Fixing a diagnosed problem

For a limited set of well-supported diagnoses, ReproBisect can propose conservative fixes:

```bash
reprobisect fix .
```

To test the proposed fix:

```bash
reprobisect fix . --verify
```

Current rule-based fixes include cases involving:

* compiler path remapping
* `SOURCE_DATE_EPOCH`
* source/archive mtime normalization
* umask/mode normalization

Fix verification happens in a temporary copy of the project.
**ReproBisect does not silently modify your working tree.**
If it cannot identify a conservative patch point, it refuses to invent one.

---

## Uncontrolled nondeterminism

ReproBisect first asks whether the canonical baseline reproduces itself.
If repeated baseline builds already differ, one-variable causal experiments are not trustworthy.
In that case ReproBisect reports uncontrolled nondeterminism instead of manufacturing a root-cause diagnosis.
This distinction is important for races, random seeds, unordered parallel work, external services, and other stochastic effects.

---

## Parallelism and stochastic builds

CPU/parallelism effects can be tested explicitly:

```toml
[experiments]
stochastic_runs = 8
stochastic_alpha = 0.05

[experiments.dimensions]
cpu_count = true
```

ReproBisect uses matched baseline/variant trials and a one-sided Fisher exact test to determine whether the intervention measurably changes the artifact-change rate.
This can show that parallelism affects the distribution of outputs.
It does not identify the internal race itself.

---

## Multi-variable interactions

Some bugs appear only when multiple variables change together.

Interaction search can be enabled with:

```toml
[experiments]
interaction_search = true
interaction_runs = 2
max_interaction_variables = 8
```

ReproBisect first identifies variables that were individually inert, then tests their combined effect and applies a ddmin-style search to find a 1-minimal observed interaction.
The resulting set is evidence about the tested variables, not a claim that no other explanation exists.

---

## Provenance and tracing

ReproBisect can optionally collect bounded, privacy-conscious provenance about the build.

Examples include:

* source Git commit and dirty state
* dependency lock/manifest fingerprints
* resolved container image identity
* best-effort toolchain versions
* tool invocation counts
* aggregate dependency-cache fingerprints
* redacted network syscall summaries
* process-aware file-input/output relationships

Optional tracing:

```toml
[experiments]
network_trace = true
file_input_trace = true
```

Raw traces remain ephemeral.

Persisted evidence intentionally avoids recording data such as:

* exact network endpoints
* command-line arguments from traced processes
* raw container PIDs
* dependency-cache paths
* arbitrary temporary paths
* raw syscall traces

These signals are provenance and correlation evidence. They are **not byte-level taint tracking**.

---

## Machine-readable evidence

Use:

```bash
reprobisect check . --format json
```

for automation or downstream analysis.

Exit codes:

| Code | Meaning                                                              |
| ---: | -------------------------------------------------------------------- |
|  `0` | operation-specific success                                           |
|  `1` | completed diagnostic operation with a finding or inconclusive result |
|  `2` | command-line usage error                                             |
|  `5` | operational/internal error                                           |

Automation should inspect the JSON report status rather than relying only on the process exit code.

Persisted evidence has explicit schemas and compatibility handling. See:

* [`docs/evidence-compatibility.md`](docs/evidence-compatibility.md)
* [`docs/architecture.md`](docs/architecture.md)

---

## Current scope

ReproBisect 1.0 focuses on:

* Linux
* Docker and Podman
* containerized builds
* an explicit build command
* one or more declared output artifacts

It does not currently promise to:

* automatically understand every build system
* prove full build hermeticity
* trace individual bytes through a process
* identify arbitrary nondeterministic code inside your program
* replace detailed artifact diffing
* guarantee that every possible environmental variable has been tested

A diagnosis means that, under the controlled experiment that was actually performed, changing this tested input repeatedly changed the observed artifact, and the configured confirmation criteria were satisfied. This is deliberately narrower than “we found every cause of nondeterminism.”

---

## Validation

ReproBisect's release qualification includes:

* Rust MSRV and current stable compiler checks/tests
* locked dependency resolution
* Docker acceptance tests
* Podman acceptance tests
* synthetic reproducibility fixtures
* real-world builds pinned to immutable upstream commits
* deterministic Linux release packaging

The real-world corpus currently contains seven cases spanning C/C++, Rust, Python packaging, and OpenSBI-based build scenarios.

See:

* [`corpus/README.md`](corpus/README.md)
* [`docs/real-world-validation.md`](docs/real-world-validation.md)
* [`docs/release-qualification.md`](docs/release-qualification.md)

---

## Documentation

For the details intentionally kept out of this README:

* [Architecture](docs/architecture.md)
* [Experiment model](docs/experiment-model.md)
* [Evidence compatibility](docs/evidence-compatibility.md)
* [Real-world validation](docs/real-world-validation.md)
* [Release qualification](docs/release-qualification.md)
* [Implementation status](docs/implementation-status.md)
* [Detailed problem statement and original specification](docs/problem-statement-and-v0.1-spec.md)

---

## Found a build ReproBisect cannot explain?

That is particularly useful.

If you have a real build that changes across machines or environments and ReproBisect:

* misses the cause
* produces a false diagnosis
* becomes inconclusive unexpectedly
* cannot represent the relevant environment difference
* fails on an artifact/build system we should support

please open an issue with a minimized reproducer if possible.
Real unexplained builds are the most useful input for deciding what ReproBisect should support next.

---

## Security and privacy

See [`SECURITY.md`](SECURITY.md) for the supported-version and vulnerability-reporting policy.
ReproBisect deliberately bounds persisted logs and tracing evidence and avoids persisting several classes of potentially sensitive runtime details. Review the provenance/tracing documentation before enabling those features on confidential builds.

---

## Contributing

Bug reports, minimized non-reproducible builds, new regression fixtures, documentation improvements, and narrowly scoped feature proposals are welcome.
For significant new experiment dimensions or evidence-schema changes, open an issue first so the causal model and compatibility implications can be discussed before implementation.

---

## License

See [`LICENSE`](LICENSE).
