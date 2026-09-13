# ReproBisect — Problem Statement and v0.1 Engineering Specification

> **Working name:** ReproBisect
> **Status:** Pre-implementation design specification
> **Target:** v0.1
> **Primary platform:** Linux, containerized builds
> **Core principle:** Diagnose non-reproducible builds through controlled, evidence-producing experiments rather than heuristics alone.

---

## 1. Executive Summary

A reproducible build is a build process that, when given equivalent source code and declared inputs, produces byte-for-byte identical output artifacts.

The software ecosystem already has good tools for answering:

- **Are these two artifacts different?**
- **Where do their bytes or metadata differ?**
- **How should projects generally make builds more reproducible?**

The harder question remains substantially more manual:

> **Why are these two builds different, and which environmental input caused the difference?**

Today, diagnosing a reproducibility failure often requires an expert to inspect artifact diffs, hypothesize possible causes, modify the build environment, rebuild repeatedly, and reason about which environmental variables are causally responsible.

**ReproBisect** aims to automate this diagnostic loop.

Given a project and a build definition, ReproBisect will:

1. perform controlled rebuilds in isolated environments;
2. establish whether the build is reproducible under the tested conditions;
3. distinguish repeat-run nondeterminism from environment-sensitive behavior;
4. perturb relevant environmental variables systematically;
5. compare resulting artifacts;
6. minimize the set of variables required to reproduce the difference;
7. inspect artifact-specific evidence;
8. report experimentally supported causal factors;
9. recommend deterministic remediations; and
10. eventually verify proposed fixes by rebuilding.

The intended mental model is:

> **`git bisect`, but over build environments rather than commits.**

More precisely, ReproBisect is a **causal debugger for build systems**.

---

# 2. Problem Statement

## 2.1 The underlying problem

Consider a build process

\[
B : (S, E) \rightarrow A
\]

where:

- \(S\) is the source tree and declared build inputs;
- \(E\) is the build environment;
- \(B\) is the build procedure;
- \(A\) is the resulting artifact or artifact set.

For two environments \(E_1\) and \(E_2\), a reproducibility failure occurs when:

\[
A_1 = B(S,E_1)
\]

and

\[
A_2 = B(S,E_2)
\]

but

\[
A_1 \ne A_2
\]

under the chosen artifact-equivalence criterion.

For v0.1, equivalence means:

\[
\operatorname{SHA256}(A_1)=\operatorname{SHA256}(A_2)
\]

for every declared output artifact.

The practical diagnostic problem is:

> Given \(S\), \(B\), \(E_1\), and \(E_2\), identify the smallest experimentally supported subset of environmental differences that explains the observed artifact difference.

Let the environmental variables be

\[
E = \{e_1,e_2,\dots,e_n\}.
\]

We want to identify a small set

\[
C \subseteq E
\]

such that intervening on the variables in \(C\) changes the artifact in the observed direction, while removing irrelevant variables from \(C\) causes the effect to disappear.

This is not merely a binary diff problem. It is an **experimental causal diagnosis problem**.

---

## 2.2 What developers experience today

A typical failure looks like this:

```text
Build A:
SHA256 = 4a51...

Build B:
SHA256 = 98fd...

Source commit:
identical
```

A binary comparison may then reveal:

```text
DWARF debug section differs.
Embedded paths differ:

/tmp/build-123/project/src/foo.c
/tmp/build-917/project/src/foo.c
```

The developer must manually infer that the changing build path is relevant, discover the corresponding compiler option, change the build, and rebuild again.

A desired ReproBisect diagnosis is instead:

```text
Reproducibility failure detected.

Observed causal factor:
  build_path

Evidence:
  Baseline path:      /workspace/run-a/project
  Intervention path:  /workspace/run-b/project
  Artifact changed:   yes
  Other controlled variables held constant: yes

Artifact evidence:
  ELF .debug_info contains absolute source/build paths.

Likely remediation:
  GCC/Clang:
    -ffile-prefix-map=$PWD=.

Confidence:
  high

Verification:
  proposed fix not yet applied
```

---

# 3. Primary Product Goal

The primary goal of ReproBisect v0.1 is:

> **Automatically diagnose common causes of byte-level non-reproducibility in Linux containerized builds using controlled interventions and artifact-aware evidence.**

The v0.1 product should convert the workflow from:

```text
detect mismatch
    ↓
inspect manually
    ↓
form hypothesis
    ↓
edit environment
    ↓
rebuild
    ↓
inspect again
    ↓
repeat
```

into:

```text
reprobisect check .
    ↓
controlled rebuilds
    ↓
automatic interventions
    ↓
artifact analysis
    ↓
causal diagnosis
    ↓
suggested remediation
```

---

# 4. v0.1 Scope

## 4.1 Supported operating environment

v0.1 targets:

- Linux hosts;
- Docker as the first container backend;
- projects that can be built using an explicit shell command;
- deterministic declaration of output artifact paths;
- generic command-driven C/C++/Rust/etc. builds;
- byte-for-byte artifact equality.

The project should not depend on recognizing a specific build system.

For example, all of the following should be valid:

```toml
command = ["sh", "-lc", "make -j1"]
```

```toml
command = ["sh", "-lc", "cargo build --release"]
```

```toml
command = ["sh", "-lc", "cmake -S . -B build && cmake --build build"]
```

The build command is supplied by the user. ReproBisect controls the environment around it.

---

## 4.2 Initial perturbation dimensions

The first release should be able to test at least:

1. wall-clock / build time;
2. `SOURCE_DATE_EPOCH`;
3. timezone;
4. locale;
5. hostname;
6. username;
7. source directory path;
8. build directory path;
9. selected environment variables;
10. filesystem mtimes;
11. filesystem enumeration / archive member ordering where controllable;
12. process parallelism / CPU count;
13. umask.

Later releases may expand into:

- compiler version;
- linker version;
- dependency versions;
- kernel version;
- architecture;
- filesystem implementation;
- network-derived inputs;
- package-manager metadata;
- CPU feature detection.

---

## 4.3 Initial artifact classes

v0.1 should support:

### Generic files

For every file:

- SHA-256;
- file size;
- MIME/type detection;
- cheap binary difference statistics.

### ELF binaries

Detect and inspect at minimum:

- debug information;
- source/build paths;
- build IDs;
- sections;
- symbol tables where useful;
- embedded strings;
- timestamp-like metadata where identifiable.

### Archives

At minimum:

- `.tar`
- `.tar.gz`
- `.zip`
- static libraries such as `.a`

Inspect:

- member ordering;
- member timestamps;
- uid/gid;
- permission metadata;
- path names;
- compression-related metadata where observable.

Additional analyzers can be added without changing the causal engine. The current implementation also carries bounded semantic summaries for supported package/container/executable formats and compares those summaries field-by-field after a byte change. Semantic parsing never executes artifact content and is subject to explicit file/member/section budgets.

---

# 5. Explicit Non-Goals for v0.1

The following are intentionally **not** required for the first release.

## 5.1 No universal build-system inference

ReproBisect will not initially attempt to determine automatically whether a project uses:

- CMake;
- Meson;
- Bazel;
- Gradle;
- Cargo;
- npm;
- Nix;
- Make;
- custom scripts.

The user supplies a build command.

---

## 5.2 No Kubernetes platform

The first version is a local CLI.

No:

- Kubernetes operator;
- distributed scheduler;
- fleet control plane;
- hosted dashboard.

---

## 5.3 No AI-generated root causes

The core diagnosis engine must be deterministic and evidence-backed.

An LLM may later help:

- explain evidence;
- search build files for likely fix locations;
- propose patches;
- summarize diagnoses.

It must not be the source of truth for whether a variable caused an observed difference.

---

## 5.4 No automatic silent source modifications

v0.1 may suggest fixes.

A future `reprobisect fix` command may generate a patch.

The tool must not silently mutate a user's source tree.

---

## 5.5 No claim of universal causality

ReproBisect can establish:

> "Under the interventions ReproBisect executed, changing variable X changed the resulting artifact while the tested controls were held constant."

It must not overstate that result as:

> "X is the only possible cause in every build environment."

The distinction matters.

---

## 5.6 No semantic-equivalence proof

v0.1 uses byte-for-byte equality.

Two binaries that differ only in debug metadata are still reported as non-reproducible.

A later system may distinguish:

- bitwise equivalence;
- package-level equivalence;
- executable semantic equivalence.

That is outside the first release.

---

# 6. Core Design Principle: Experimental Diagnosis

ReproBisect should treat a build failure as a controlled scientific experiment.

A diagnosis is not produced merely because a suspicious string appears in a binary.

Instead:

1. observe a reproducibility failure;
2. formulate one or more hypotheses;
3. intervene on a suspected environmental variable;
4. rebuild;
5. measure whether the artifact changes;
6. correlate artifact-level evidence with the intervention;
7. repeat or minimize if necessary;
8. report only evidence-supported conclusions.

---

# 7. The Critical Distinction: Environmental Sensitivity vs Intrinsic Nondeterminism

A major conceptual issue is that not every changing artifact is caused by a stable environmental difference.

Suppose repeated builds under nominally identical environment \(E_0\) produce:

\[
A_1 \ne A_2
\]

even though:

\[
E_1 = E_2 = E_0.
\]

This suggests uncontrolled or stochastic nondeterminism.

Possible causes include:

- race conditions;
- parallel build ordering;
- filesystem enumeration order;
- randomness;
- uninitialized data;
- network state;
- non-hermetic dependencies;
- generated identifiers;
- uncontrolled temporary paths.

Therefore ReproBisect must first determine whether the build is **stable under repetition**.

---

## 7.1 Control-run protocol

Before attributing differences to environmental variables, ReproBisect should perform repeated control builds.

Example:

```text
Control environment E0

run 1 → A1
run 2 → A2
run 3 → A3
```

If:

```text
A1 = A2 = A3
```

then the environment is sufficiently stable for deterministic intervention analysis.

If:

```text
A1 != A2
```

the build is already nondeterministic under the control condition.

The tool should then switch diagnosis mode.

Example output:

```text
Intrinsic nondeterminism detected.

The build produced different artifacts across repeated runs
without an intentional environment change.

Candidate classes to investigate:
  - parallelism
  - filesystem ordering
  - randomness
  - timestamps
  - generated temporary paths

Environment-variable causal attribution has been deferred until
a stable control condition is established.
```

---

# 8. Causal Model

## 8.1 Variables

A run is modeled as:

\[
R = (S,B,E,A,M)
\]

where:

- \(S\): source snapshot;
- \(B\): build command and build definition;
- \(E\): controlled environment manifest;
- \(A\): produced artifacts;
- \(M\): run metadata and observations.

An intervention is:

\[
I(e_i := v)
\]

meaning that environmental variable \(e_i\) is intentionally assigned value \(v\), while all other controlled variables are held constant.

---

## 8.2 Single-variable intervention

For a variable \(x\):

```text
Control:
  E[x] = x0
  → artifact A0

Intervention:
  E[x] = x1
  → artifact A1
```

If:

```text
A0 != A1
```

and repeated controls are stable, then changing \(x\) is an observed causal factor under the tested intervention.

---

## 8.3 Interaction effects

Some failures require multiple variables.

Example:

```text
timezone alone   → no difference
locale alone     → no difference
timezone+locale  → difference
```

Therefore ReproBisect cannot rely only on one-variable-at-a-time testing.

Once individual tests are exhausted or multiple variables differ between known environments, the engine should support **delta debugging** over variable sets.

---

# 9. Delta Debugging / Cause Minimization

Suppose the environmental difference set is:

\[
D = \{d_1,d_2,\ldots,d_n\}.
\]

A naive exhaustive search requires:

\[
2^n
\]

combinations.

This becomes impractical quickly.

ReproBisect should instead use a ddmin-style algorithm to find a 1-minimal failure-inducing subset.

Conceptually:

```text
D = {timezone, locale, hostname, build_path, username, umask}

test first half
test second half
retain failure-inducing subset
split again
repeat
```

The result might be:

```text
Minimal observed causal set:
  {build_path}
```

or:

```text
Minimal observed causal set:
  {locale, filesystem_order}
```

The tool must call this a **minimal observed causal set under the tested experiment space**, not a mathematically guaranteed globally minimal real-world cause.

---

# 10. High-Level Architecture

```text
                    ┌──────────────────────┐
                    │         CLI          │
                    └──────────┬───────────┘
                               │
                               ▼
                    ┌──────────────────────┐
                    │ Configuration Loader │
                    └──────────┬───────────┘
                               │
                               ▼
                    ┌──────────────────────┐
                    │ Diagnosis Controller │
                    └──────┬─────────┬─────┘
                           │         │
               ┌───────────┘         └───────────┐
               ▼                                 ▼
     ┌───────────────────┐             ┌───────────────────┐
     │ Experiment Engine │             │  Analyzer Engine  │
     └─────────┬─────────┘             └─────────┬─────────┘
               │                                 │
               ▼                                 ▼
     ┌───────────────────┐             ┌───────────────────┐
     │   Runner Backend  │             │ Artifact Analyzers│
     │     (Docker)      │             │ ELF/archive/etc.  │
     └─────────┬─────────┘             └─────────┬─────────┘
               │                                 │
               ▼                                 ▼
     ┌───────────────────┐             ┌───────────────────┐
     │   Build Results   │────────────▶│ Evidence Records  │
     └───────────────────┘             └─────────┬─────────┘
                                                │
                                                ▼
                                      ┌───────────────────┐
                                      │ Diagnosis / Fixes │
                                      └───────────────────┘
```

---

# 11. Architectural Components

## 11.1 CLI layer

Responsibilities:

- parse commands and flags;
- load configuration;
- display progress;
- produce human-readable reports;
- support machine-readable JSON output;
- map exit codes.

The CLI must remain a thin layer over the core engine.

---

## 11.2 Configuration loader

Reads `.reprobisect.toml`.

Responsibilities:

- validate build command;
- validate image;
- resolve output paths;
- normalize environment values;
- construct a `BuildSpec`.

---

## 11.3 Runner

The runner executes one build under one explicit environment.

The current local OCI backend is:

```text
OciRunner
  ├─ Docker
  └─ Podman
```

Future non-OCI backends may include Nix, native namespaces, and remote builders.

The core engine depends on a runner interface rather than directly on one container-runtime command line.

Conceptual interface:

```rust
trait Runner {
    fn run(&self, spec: &BuildSpec, env: &ExperimentEnvironment)
        -> Result<BuildRun>;
}
```

---

## 11.4 Experiment engine

Responsibilities:

- create baseline runs;
- repeat control runs;
- generate perturbations;
- schedule interventions;
- compare outcomes;
- perform ddmin-style minimization;
- stop experiments when enough evidence exists;
- keep an explicit experiment graph.

---

## 11.5 Artifact comparator

Responsibilities:

- hash artifacts;
- match corresponding outputs;
- identify changed artifacts;
- compute cheap binary statistics;
- dispatch to specialized analyzers.

---

## 11.6 Analyzer engine

Artifact-specific analyzers produce structured findings.

Examples:

```text
AbsolutePathFinding
TimestampFinding
ArchiveMemberOrderFinding
LocaleSensitiveTextFinding
ElfBuildIdFinding
DebugPathFinding
EnvironmentLeakFinding
```

A finding alone is not necessarily a diagnosis.

A diagnosis requires combining:

```text
experimental effect
+
artifact evidence
+
known remediation rule
```

---

## 11.7 Diagnosis engine

Produces ranked diagnoses.

Example conceptual structure:

```text
Diagnosis
├── causal_variables
├── affected_artifacts
├── experimental_evidence
├── artifact_evidence
├── confidence
├── explanation
└── remediation_candidates
```

---

# 12. Data Model

The internal model should be explicit enough that every user-visible diagnosis can be traced back to build runs.

## 12.1 `BuildSpec`

```text
BuildSpec
  project_root
  image
  command
  outputs
  base_environment
  working_directory
  timeout
```

---

## 12.2 `ExperimentEnvironment`

```text
ExperimentEnvironment
  timezone
  locale
  hostname
  username
  source_path
  build_path
  umask
  source_date_epoch
  cpu_count
  environment_variables
  filesystem_mtime_policy
  other controlled values
```

---

## 12.3 `BuildRun`

```text
BuildRun
  run_id
  experiment_id
  source_digest
  environment_manifest
  command
  start_time
  duration
  exit_status
  stdout_log
  stderr_log
  artifact_records
  runner_metadata
```

---

## 12.4 `ArtifactRecord`

```text
ArtifactRecord
  logical_name
  path
  sha256
  size
  detected_type
  metadata
```

---

## 12.5 `Intervention`

```text
Intervention
  variable
  control_value
  intervention_value
  reason
```

or for combined experiments:

```text
InterventionSet
  interventions[]
```

---

## 12.6 `Evidence`

```text
Evidence
  evidence_id
  run_ids
  observation
  analyzer
  strength
  raw_details
```

---

## 12.7 `Diagnosis`

```text
Diagnosis
  diagnosis_id
  title
  causal_variables[]
  evidence[]
  confidence
  affected_artifacts[]
  remediation[]
  limitations[]
```

---

# 13. Run Manifest

Every build run should emit an immutable run manifest.

Example:

```json
{
  "run_id": "01J...",
  "source_digest": "sha256:...",
  "image": "debian:bookworm",
  "command": ["sh", "-lc", "make"],
  "environment": {
    "TZ": "UTC",
    "LANG": "C.UTF-8",
    "HOSTNAME": "reprobisect-a",
    "USER": "builder",
    "SOURCE_DATE_EPOCH": null,
    "source_path": "/workspace/src-a",
    "build_path": "/workspace/build-a",
    "cpu_count": 1,
    "umask": "0022"
  },
  "outputs": [
    {
      "path": "build/app",
      "sha256": "..."
    }
  ]
}
```

This is essential because ReproBisect must be able to justify:

> "These were the variables held constant, and this was the one we intentionally changed."

Without a precise manifest, causal claims become unreliable.

---

# 14. v0.1 Experiment Protocol

## Phase A — Preparation

1. load config;
2. snapshot or digest the source tree;
3. resolve container image;
4. validate build command;
5. validate output declarations;
6. produce the canonical baseline environment.

---

## Phase B — Baseline build

Run:

```text
baseline-1
```

If the build fails:

```text
status = build-failed
```

and stop reproducibility diagnosis.

ReproBisect is not primarily a general build-error debugger.

---

## Phase C — Control repetition

Run at least one additional build under an equivalent controlled environment:

```text
baseline-2
```

Optionally:

```text
baseline-3
```

Compare artifact hashes.

### Case 1: identical

Proceed to environmental interventions.

### Case 2: different

Enter intrinsic-nondeterminism mode.

---

## Phase D — One-variable interventions

Test high-value variables individually.

Suggested initial ordering:

```text
1. build/source path
2. build time / SOURCE_DATE_EPOCH
3. timezone
4. locale
5. hostname
6. username
7. umask
8. CPU count / parallelism
9. filesystem mtimes
10. selected environment variables
```

The ordering may later become adaptive.

---

## Phase E — Artifact analysis

For every intervention that changes the output:

1. identify changed artifact(s);
2. perform generic binary analysis;
3. dispatch to specialized analyzers;
4. link observed differences to the intervention.

Example:

```text
Intervention:
  build_path

Artifact effect:
  app SHA changed

Analyzer finding:
  ELF .debug_info contains the intervened path value
```

This is stronger evidence than either observation alone.

---

## Phase F — Interaction search

If:

- known environments differ but individual perturbations do not reproduce the effect; or
- multiple variables appear causally relevant;

run grouped interventions and ddmin-style minimization.

---

## Phase G — Diagnosis synthesis

Produce:

- causal variable/set;
- artifact-level evidence;
- confidence;
- remediation rule;
- known limitations.

---

# 15. Confidence Model

The tool should avoid opaque numerical pseudo-precision.

v0.1 can use categorical confidence.

## High confidence

Example conditions:

- repeated controls are stable;
- intervention reliably changes artifact;
- reverting intervention restores baseline;
- artifact contains direct evidence related to changed value;
- remediation is well-known and specific.

---

## Medium confidence

Example:

- intervention reliably changes artifact;
- artifact difference is compatible with the variable;
- direct value embedding is not observed.

---

## Low confidence

Example:

- effect appears intermittently;
- control stability is imperfect;
- multiple confounded variables remain;
- analyzer evidence is weak.

Low-confidence findings should be reported as hypotheses, not root causes.

---

# 16. Fix Verification

A remediation recommendation becomes substantially stronger if ReproBisect can verify it.

Future workflow:

```text
cause detected
   ↓
fix candidate generated
   ↓
patched build executed
   ↓
original perturbation repeated
   ↓
artifacts become identical
```

Then the diagnosis can report:

```text
Fix verified experimentally.
```

The long-term product loop is:

> **Detect → Diagnose → Explain → Patch → Verify**

For v0.1, **Detect → Diagnose → Explain** is sufficient.

---

# 17. CLI Contract

The initial CLI should remain small.

## 17.1 Primary command

```bash
reprobisect check .
```

Behavior:

1. load `.reprobisect.toml`;
2. run control builds;
3. detect reproducibility status;
4. diagnose automatically if outputs differ;
5. print a report.

The user should not need to know whether `check` internally enters diagnostic mode.

---

## 17.2 Optional explicit command

```bash
reprobisect diagnose .
```

This may be added if it becomes useful to diagnose from previously captured build results.

It is not required to make the v0.1 interface useful.

---

## 17.3 Initialization

```bash
reprobisect init
```

Creates a starter:

```text
.reprobisect.toml
```

Potentially by asking minimal interactive questions or writing an example configuration.

---

## 17.4 JSON output

```bash
reprobisect check . --format json
```

Required eventually for:

- CI;
- GitHub Actions;
- downstream tooling;
- regression tests.

---

## 17.5 Verbose experiment output

```bash
reprobisect check . -v
```

Example:

```text
[control] build 1 ........ PASS
[control] build 2 ........ DIFFERENT

Intrinsic nondeterminism detected.

Testing:
  parallelism ............ EFFECT OBSERVED
  filesystem mtimes ...... no effect
  timezone ............... no effect

Analyzing:
  build/app .............. ELF
```

---

# 18. Configuration Contract

Initial configuration file:

```text
.reprobisect.toml
```

Minimal example:

```toml
[build]
image = "debian:bookworm"
command = ["sh", "-lc", "make"]
outputs = ["build/app"]
```

Expanded example:

```toml
[build]
image = "rust:1.89-bookworm"
command = ["sh", "-lc", "cargo build --release"]
outputs = ["target/release/myapp"]
timeout_seconds = 900
log_capture_max_bytes = 1048576

[environment]
TZ = "UTC"
LANG = "C.UTF-8"

[experiments]
control_runs = 2

[experiments.variables]
timezone = true
locale = true
hostname = true
username = true
source_path = true
build_path = true
source_date_epoch = true
parallelism = true
umask = true
filesystem_mtime = true
```

The config schema should start narrow and expand only when actual use cases demand it.

---

# 19. Example User Experience

## Reproducible project

```text
$ reprobisect check .

ReproBisect v0.1

Source:
  2c8daef...

Build:
  docker image: debian:bookworm
  command: make

Control builds:
  run 1  7b42a7...
  run 2  7b42a7...

Result:
  REPRODUCIBLE under tested conditions

Artifacts:
  build/app  sha256:7b42a7...

Experiments executed:
  2
```

---

## Build-path failure

```text
$ reprobisect check .

Reproducibility failure detected.

Artifact:
  build/app

Control stability:
  stable

Observed causal factor:
  build_path

Experiment:
  /workspace/a/project → sha256:4a51...
  /workspace/b/project → sha256:98fd...

Artifact evidence:
  ELF DWARF information contains the absolute build path.

Likely remediation:
  GCC/Clang:
    -ffile-prefix-map=$PWD=.

Confidence:
  HIGH
```

---

## Intrinsic nondeterminism

```text
$ reprobisect check .

Reproducibility failure detected.

Control runs:
  run 1 → 1041...
  run 2 → af20...
  run 3 → 39cd...

The artifact changes without an intentional environmental intervention.

Classification:
  INTRINSIC / UNCONTROLLED NONDETERMINISM

Strongest tested factor:
  parallelism

Evidence:
  cpu_count=1 → stable across 3 runs
  cpu_count=8 → unstable across 3 runs

Next investigation:
  ordering/race-sensitive build step

Confidence:
  MEDIUM
```

---

# 20. Repository Structure

Avoid premature micro-crate fragmentation.

A good initial Rust repository is:

```text
reprobisect/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE
├── .github/
│   └── workflows/
│       └── ci.yml
├── docs/
│   ├── problem-statement-and-v0.1-spec.md
│   ├── architecture.md
│   └── experiment-model.md
├── src/
│   ├── main.rs
│   ├── cli.rs
│   ├── config.rs
│   ├── model.rs
│   ├── engine/
│   │   ├── mod.rs
│   │   ├── control.rs
│   │   ├── interventions.rs
│   │   ├── ddmin.rs
│   │   └── diagnosis.rs
│   ├── runner/
│   │   ├── mod.rs
│   │   └── docker.rs
│   ├── artifact/
│   │   ├── mod.rs
│   │   ├── generic.rs
│   │   ├── elf.rs
│   │   └── archive.rs
│   ├── evidence.rs
│   └── report.rs
├── tests/
│   ├── cli.rs
│   ├── ddmin.rs
│   └── integration.rs
└── fixtures/
    ├── timestamp-c/
    ├── absolute-path-c/
    ├── locale-sort/
    ├── timezone-output/
    ├── environment-leak/
    ├── archive-metadata/
    ├── hostname-embed/
    └── parallel-order/
```

Once interfaces become stable, components may be split into crates.

Doing so on day one would add architectural ceremony without improving the first implementation.

---

# 21. Why Rust

Rust is a strong default implementation language because ReproBisect is expected to become a systems-oriented standalone CLI.

Useful properties:

- single-binary distribution;
- strong typed internal model;
- safe process/file handling;
- good CLI ecosystem;
- good parsing ecosystem;
- straightforward Docker process orchestration;
- suitable foundation for future ELF/archive/system-level work;
- potential reuse if native sandboxing or runtime instrumentation is added later.

Python would make a throwaway prototype faster, but the project is sufficiently systems-heavy that a Rust core is likely to avoid a later rewrite.

---

# 22. Benchmark / Fixture Suite

The benchmark corpus is part of the product, not merely test scaffolding.

Every supported diagnosis should have a minimal intentionally broken project with:

1. known nondeterminism;
2. known causal variable;
3. known expected evidence;
4. known expected remediation.

This gives ReproBisect a measurable correctness target.

---

## 22.1 Fixture: embedded timestamp

### Broken behavior

C program:

```c
printf("%s %s\n", __DATE__, __TIME__);
```

or build-generated timestamp data.

### Expected result

Changing build time changes artifact.

### Expected diagnosis

```text
timestamp-sensitive output
```

### Possible remediation

Use a stable timestamp derived from `SOURCE_DATE_EPOCH`, or remove build-time embedding.

---

## 22.2 Fixture: absolute build/source path

### Broken behavior

Debug symbols or generated source embed:

```text
/home/user/project
```

### Expected intervention

Change source/build directory.

### Expected diagnosis

```text
absolute path embedded in artifact
```

### Likely fix

```text
-ffile-prefix-map
-fdebug-prefix-map
```

where applicable.

---

## 22.3 Fixture: locale-sensitive sorting

### Broken behavior

Build script sorts strings according to process locale.

### Intervention

```text
LANG=C
```

versus another locale.

### Expected diagnosis

```text
locale-sensitive generation
```

---

## 22.4 Fixture: timezone-sensitive output

### Broken behavior

A generated file serializes local time.

### Intervention

```text
TZ=UTC
```

versus another timezone.

### Expected diagnosis

```text
timezone-sensitive generation
```

---

## 22.5 Fixture: environment-variable leak

### Broken behavior

```text
BUILD_USER
CI_JOB_ID
HOME
CUSTOM_VERSION
```

is embedded in generated metadata.

### Expected diagnosis

The specific environment variable is observed to change output.

---

## 22.6 Fixture: hostname embedding

### Broken behavior

Build metadata includes:

```text
hostname()
```

### Expected intervention

Change container hostname.

### Expected diagnosis

```text
hostname-sensitive artifact
```

---

## 22.7 Fixture: archive metadata

### Broken behavior

Archive records member:

- mtimes;
- uid/gid;
- unstable ordering.

### Expected diagnosis

```text
archive metadata/order non-reproducibility
```

---

## 22.8 Fixture: parallel ordering

### Broken behavior

Parallel build combines generated outputs in completion order.

### Intervention

```text
CPU_COUNT=1
```

versus:

```text
CPU_COUNT=8
```

or repeated multithreaded builds.

### Expected diagnosis

```text
parallelism-sensitive / ordering nondeterminism
```

---

# 23. Fixture Contract

Each fixture should contain something like:

```text
fixture.toml
```

Example:

```toml
name = "absolute-path-c"
expected_status = "non_reproducible"
expected_cause = "build_path"
expected_confidence = "high"

[build]
image = "gcc:latest"
command = ["sh", "-lc", "make"]
outputs = ["app"]
```

The integration test can then assert that the diagnosis engine returns the expected cause.

This turns the benchmark suite into an executable specification.

---

# 24. diffoscope Integration

`diffoscope` is extremely useful but should be an **optional accelerator/analyzer**, not a mandatory architectural dependency.

Reasons:

- installation can be heavy;
- startup/runtime cost can be significant;
- ReproBisect should support fast internal checks;
- structured internal analyzers are easier to correlate directly with interventions.

Suggested strategy:

```text
cheap hash comparison
    ↓
internal type detection
    ↓
internal targeted analyzer
    ↓
diffoscope if installed / requested / useful
```

ReproBisect can ingest diffoscope output as additional evidence.

---

# 25. Diagnostic Rule Model

A deterministic rule can combine experimental and artifact evidence.

Example conceptual rule:

```text
IF
    intervention.variable == build_path
AND artifact_changed == true
AND ELF debug info contains intervention path
THEN
    diagnosis = AbsoluteDebugPath
    confidence = high
    remediation = file-prefix-map
```

Another:

```text
IF
    repeated control builds differ
AND cpu_count=1 becomes stable
AND cpu_count>1 remains unstable
THEN
    diagnosis = ParallelismSensitiveNondeterminism
    confidence = medium/high depending on repetition count
```

Rules must remain inspectable and testable.

---

# 26. Experimental Repetition

One build is weak evidence.

For deterministic variables, a practical v0.1 protocol can be:

```text
control:      2 runs
intervention: 1 run
confirmation: optional second intervention run
```

For flaky/nondeterministic effects, later releases should pair repetition with an explicit statistical contract rather than treating "different at least once" as strong causal evidence.

The implemented CPU/parallelism probe uses fixed-size interleaved baseline/variant samples. Each trial is categorized by whether its artifact signature differs from the original stable baseline, and a one-sided Fisher exact test evaluates enrichment of changed outcomes under the CPU variant. The configured `stochastic_alpha` is persisted with the p-value.

This statistical layer remains subordinate to the control-stability invariant: if any matched baseline reference changes, causal promotion is refused and the report becomes `UNCONTROLLED_NONDETERMINISM`. The test is deliberately categorical and does not infer the mechanism of a race or provide multiple-hypothesis correction across arbitrary user-defined experiment families.

---

# 27. Caching Policy

Build diagnosis is expensive, so caching is attractive.

However, careless caching can mask the very nondeterminism being investigated.

Safe caching may include:

- container image layers;
- downloaded immutable toolchains;
- source snapshot materialization.

Unsafe default caching includes:

- build outputs;
- generated intermediate files;
- mutable dependency caches;
- arbitrary previous workspace state.

Therefore the v0.1 default should favor **fresh build workspaces**.

Performance optimization comes after diagnostic correctness.

---

# 28. Isolation Model

Each run must receive a fresh isolated workspace.

The runner should explicitly control:

```text
source mount/copy path
build path
hostname
environment
timezone
locale
user
umask
CPU limit
network policy
```

Some variables are themselves experiment targets.

The runner must therefore avoid accidentally fixing them globally in a way that makes them impossible to perturb.

---

# 29. Source Snapshot Semantics

All compared builds must operate on the same logical source snapshot.

At minimum record:

```text
git commit, if available
dirty-tree status
source-tree digest
```

A source-tree digest is important because two working trees at the same commit may differ because of uncommitted/generated files.

---

# 30. Network Policy

Network access is a major confounder.

v0.1 can initially allow network access for practical compatibility, but the run manifest should record the policy.

A stronger future mode:

```toml
network = "disabled"
```

or:

```toml
network = "recorded"
```

Long term, detecting undeclared network inputs is highly valuable.

It is not required for the first working release.

---

# 31. Error Taxonomy

The CLI should distinguish:

```text
BUILD_FAILED
REPRODUCIBLE
NON_REPRODUCIBLE_DIAGNOSED
NON_REPRODUCIBLE_UNDIAGNOSED
INTRINSIC_NONDETERMINISM
CONFIGURATION_ERROR
RUNNER_ERROR
ANALYZER_ERROR
```

These states should also map to stable machine-readable result values.

---

# 32. Exit Codes

A tentative contract:

```text
0  reproducible / successful check
1  non-reproducible
2  configuration error
3  build failure
4  infrastructure/runner failure
5  internal error
```

The exact mapping can be finalized during implementation, but it must be stable before the first tagged release.

---

# 33. Security Considerations

ReproBisect executes arbitrary project build commands.

Therefore:

- builds should run inside isolated containers by default;
- project code must be treated as untrusted;
- host filesystem mounts should be minimized;
- Docker socket exposure inside the build container should be prohibited;
- credentials must not be forwarded automatically;
- environment inheritance should use an explicit allowlist;
- network access should eventually be controllable;
- output parsing must not execute artifact content.

ReproBisect itself should not claim that Docker provides a perfect malicious-code sandbox.

---

# 34. Privacy / Data Handling

A local v0.1 should not upload:

- source;
- binaries;
- logs;
- environment variables;
- diagnostic output

to any external service.

This keeps the first version deployable on private codebases without introducing a cloud trust boundary.

---

# 35. Success Criteria for v0.1

v0.1 is successful if all of the following are true.

## Functional

- a user can define a generic Linux containerized build;
- ReproBisect runs fresh builds;
- it compares declared artifacts;
- it detects stable reproducibility;
- it detects intrinsic repeated-run nondeterminism;
- it tests an initial set of environmental variables;
- it diagnoses at least the benchmarked common failure classes;
- it emits human-readable evidence;
- it can emit structured JSON.

---

## Correctness

For the first benchmark corpus:

- every intentionally reproducible fixture is classified correctly;
- every intentionally non-reproducible fixture is detected;
- supported fixtures return the expected causal variable/class;
- no high-confidence cause is reported without intervention evidence.

---

## Usability

A minimal project should need no more than:

```toml
[build]
image = "..."
command = [...]
outputs = [...]
```

then:

```bash
reprobisect check .
```

---

# 36. What v0.1 Does Not Need to Prove

v0.1 does **not** need to:

- diagnose every real-world build;
- support every artifact type;
- support every package manager;
- patch projects automatically;
- run on Windows/macOS;
- replace diffoscope;
- replace Nix;
- replace Reproducible Builds infrastructure;
- prove hermeticity;
- prove semantic equivalence;
- perform distributed CI;
- use AI.

A focused tool that correctly diagnoses a small but valuable set of failures is a stronger foundation than a broad tool with unreliable conclusions.

---

# 37. Phased Implementation Roadmap

## Phase 0 — Executable benchmark specification

Build the fixture corpus first.

Deliver:

- timestamp fixture;
- path fixture;
- locale fixture;
- timezone fixture;
- env-leak fixture;
- hostname fixture;
- archive metadata fixture;
- parallelism fixture.

Each has an expected diagnosis.

This prevents the implementation from becoming feature-driven without a correctness target.

---

## Phase 1 — Build runner and hashing

Implement:

- config parsing;
- Docker runner;
- fresh workspace creation;
- source digest;
- declared output collection;
- SHA-256 comparison;
- run manifest.

At the end of Phase 1:

```bash
reprobisect check .
```

can say only:

```text
reproducible
```

or:

```text
not reproducible
```

but does so reliably.

---

## Phase 2 — Control stability

Implement repeated builds under nominally identical environments.

Classify:

```text
stable reproducibility failure
```

versus:

```text
intrinsic nondeterminism
```

This should be completed before sophisticated causal claims.

---

## Phase 3 — Environmental interventions

Implement perturbations for:

- source/build path;
- timezone;
- locale;
- hostname;
- username;
- `SOURCE_DATE_EPOCH`;
- umask;
- CPU count/parallelism;
- mtimes;
- explicit env variables.

Record every intervention.

---

## Phase 4 — Artifact analyzers

Implement:

- generic byte/string analysis;
- ELF analyzer;
- archive analyzer.

Connect analyzer evidence to experiments.

---

## Phase 5 — Diagnosis rules

Implement evidence-backed diagnosis synthesis.

Examples:

```text
build_path + DWARF path
timezone + generated text timestamp
locale + sorted textual output
hostname + embedded string
archive mtime differences
```

---

## Phase 6 — Cause minimization

Implement ddmin over environmental delta sets.

Use it when:

- multiple variables differ between target environments;
- individual tests are insufficient;
- interaction effects are suspected.

---

## Phase 7 — CI integration

Create a GitHub Action or generic CI mode.

Example:

```yaml
- uses: reprobisect/reprobisect-action@v1
```

Possible PR output:

```text
❌ Build is not reproducible

Cause:
absolute build path embedded in ELF debug information

Suggested fix:
-ffile-prefix-map
```

---

# 38. Long-Term Product Direction

The architecture should leave room for several future layers.

## 38.1 `reprobisect fix`

Generate a candidate patch.

Never apply silently.

---

## 38.2 Verified fixes

Re-run original intervention after applying patch.

Return:

```text
FIX VERIFIED
```

only when experimentally confirmed.

---

## 38.3 More runners

Docker and Podman now share the local OCI runner. Further backends may include:

- Nix;
- native namespaces;
- remote builders;
- CI providers.

---

## 38.4 More ecosystems

Artifact breadth now includes structural/semantic support for JAR, Python wheels, OCI images, DEB, Mach-O, PE/COFF, and WebAssembly. The semantic layer is bounded and non-executing; selected metadata is summarized rather than trusted or run. Further ecosystem work may include:

- APK/AAB;
- RPM;
- richer compressed JAR/wheel metadata under explicit decompression budgets;
- deeper DEB control metadata;
- richer OCI manifest/config/layer provenance;
- code-signing/notarization metadata where safe to parse.

---

## 38.5 Historical/environment comparison

Given:

```text
known-good build environment
known-bad build environment
```

automatically compute the controlled environmental delta and minimize it.

This workflow is implemented as `reprobisect compare`. Both endpoint environments are rebuilt repeatedly first. The known-good endpoint must stably succeed with one declared-artifact signature. The known-bad endpoint may stably produce a different artifact signature or stably exit non-zero. Minimization is refused for mixed/unstable endpoints.

For an artifact-producing bad endpoint, a candidate subset reproduces only when repeated builds exactly match the stable known-bad declared-artifact signature. Producing merely *some* artifact different from the known-good build is insufficient. For a build-failure bad endpoint, every repeated candidate attempt must reproduce the exact tuple `(exit_code, sha256(stdout), sha256(stderr))`; matching a generic non-zero exit code alone is insufficient. Runner/infrastructure failures are not build-failure evidence.

The resulting set is reported as a **1-minimal observed bad-environment delta under the tested values, search path, and repetition policy**. It is not a claim of globally unique real-world causality, and a matching failure signature is not proof of semantic defect identity. Controlled values in comparison manifests are evidence and may be persisted, so secrets must not be placed in those manifests.

---

## 38.6 Agent/LLM assistance

Only after deterministic evidence exists.

Potential uses:

- explain diagnosis;
- locate relevant build flags in source;
- draft a patch;
- link evidence to build scripts;
- generate a maintainer-friendly issue report.

The LLM should reason **from evidence produced by the experiment engine**, not invent causal explanations independently.

---

# 39. Key Engineering Risks

## 39.1 Arbitrary build systems

Build processes are effectively arbitrary programs.

Mitigation:

- explicit configuration;
- generic command execution;
- avoid magical build-system inference early.

---

## 39.2 Confounding introduced by ReproBisect itself

Changing container paths, mounts, timing, or workspaces can create new differences.

Mitigation:

- explicit environment manifests;
- repeat controls;
- minimal interventions;
- predictable runner behavior.

---

## 39.3 Flaky builds

A variable may appear causal because the build was already unstable.

Mitigation:

- repeated control runs;
- repeated confirmation for unstable effects;
- separate intrinsic-nondeterminism classification.

---

## 39.4 Interacting causes

Some failures occur only under variable combinations.

Mitigation:

- group testing;
- ddmin;
- no assumption that all causes are independent.

---

## 39.5 Expensive builds

A diagnosis may require many rebuilds.

Mitigation later:

- adaptive experiment ordering;
- artifact-informed hypothesis prioritization;
- safe caching;
- experiment pruning;
- reuse of completed run manifests.

Correctness comes first.

---

## 39.6 False certainty

The largest product risk is confidently naming the wrong cause.

Mitigation:

- separate findings from diagnoses;
- require intervention evidence;
- categorical confidence;
- explicit limitations;
- reproducible run manifests.

---

# 40. Design Invariants

The following should remain true as the project evolves.

1. **Every causal claim is backed by at least one recorded intervention.**
2. **Every build used as evidence has a complete environment manifest.**
3. **Control stability is checked before strong causal attribution.**
4. **Artifact heuristics alone do not establish causality.**
5. **The core diagnostic engine does not require an LLM.**
6. **Fresh workspaces are the safe default.**
7. **ReproBisect reports the scope of its experiment, not universal truth.**
8. **The simplest useful CLI remains one command.**
9. **Benchmark fixtures act as an executable specification.**
10. **Fix verification, when implemented, uses the original failing intervention.**

---

# 41. Initial Technical Decisions

For the first implementation, use:

| Area | Decision |
|---|---|
| Language | Rust |
| Runtime model | Local CLI |
| Initial runner | Docker |
| Config | TOML |
| Human output | Terminal text |
| Machine output | JSON |
| Artifact identity | SHA-256 |
| Equivalence | Byte-for-byte |
| Experiment isolation | Fresh container/workspace |
| First analyzers | Generic, ELF, archive |
| Cause search | One-variable interventions + ddmin |
| AI dependency | None |
| diffoscope | Optional integration |
| Primary target | Linux containerized builds |

---

# 42. Minimal v0.1 Promise

The product promise should remain narrow enough to be credible:

> **ReproBisect diagnoses common reproducibility failures in Linux containerized builds by automatically varying environmental inputs, rebuilding the project, and correlating artifact differences with controlled interventions.**

It should **not** initially promise:

> "ReproBisect can find the cause of any non-reproducible build."

---

# 43. Example End-to-End Internal Flow

```text
User
  |
  | reprobisect check .
  v
Load BuildSpec
  |
  v
Digest source
  |
  v
Run control build A
  |
  v
Run control build B
  |
  +---- identical ------------------------------+
  |                                             |
  |                                             v
  |                                  report reproducible
  |
  v
Different
  |
  v
repeat control / classify stability
  |
  +---- unstable -------------------------------+
  |                                             |
  |                                             v
  |                                intrinsic nondeterminism
  |                                experiment branch
  |
  v
stable difference
  |
  v
run interventions
  |
  v
compare artifacts
  |
  v
analyze changed artifacts
  |
  v
combine experiment + artifact evidence
  |
  v
minimize causal variable set
  |
  v
produce diagnosis
  |
  v
suggest remediation
```

---

# 44. Definition of Done for the First Public Demo

The first convincing public demo should show at least three cases.

## Demo A — path leakage

```bash
reprobisect check fixtures/absolute-path-c
```

and correctly identify an embedded build/source path.

---

## Demo B — timestamp leakage

```bash
reprobisect check fixtures/timestamp-c
```

and correctly identify time-sensitive output.

---

## Demo C — unstable parallel build

```bash
reprobisect check fixtures/parallel-order
```

and correctly distinguish intrinsic nondeterminism from a deterministic environmental difference.

If those three flows are rigorous, explainable, and repeatable, the core concept has been demonstrated.

---

# 45. Final Definition

ReproBisect is not primarily:

- a binary diff viewer;
- a reproducibility checklist;
- a build-system wrapper;
- a generic CI service;
- an AI debugging agent.

It is:

> **An automated experimental system for identifying the environmental causes of non-reproducible software builds.**

Its fundamental unit is not a heuristic.

Its fundamental unit is an **intervention**.

Its core output is not:

> "These files differ."

It is:

> "When we changed this controlled input, the artifact changed in this specific way; here is the evidence, here is the smallest causal set we found, and here is the likely remediation."

That is the problem this project is intended to solve.
