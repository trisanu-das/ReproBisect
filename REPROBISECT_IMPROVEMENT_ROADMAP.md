# ReproBisect — Complete Project Handoff, Current State, and Improvement Roadmap

**Snapshot date:** 2026-10-01 (read-only upstream recheck and roadmap review)  
**Repository:** https://github.com/trisanu-das/ReproBisect  
**Current public status:** Public repository; `v1.0.0` remains the published stable release. The `develop/1.1.0` branch now contains a fully qualified final `1.1.0` source tree, but `v1.1.0` has **not** been tagged or published yet.  
**Intended audience for this document:** A new human maintainer, coding agent, or LLM that needs enough context to continue development without reconstructing the entire prior conversation.

> **Read this first:** The original 21 implementation phases are complete. Upstream `develop/1.1.0` remains qualified but unpublished; the local nested checkout is an older RC snapshot, not that source. Read the October review below before executing anything. The next product slice is **safe, budgeted CI-native operation**, not a rewrite. Add an optional **typed-decision model** experiment (Laya first, Jev optional) alongside the deterministic scheduler after its safety and benchmark gates; keep generative assistance and regression-debugging generalization separate. All new commands, adapters, and tasks below are proposals unless explicitly marked implemented. This review authorizes documentation changes only—not checkout changes, builds, model installation, API inference, tagging, or publication.

---

# October 2026 review — current execution authority

This section and Part III take precedence over older sequencing and calendar estimates. Parts I–II retain the historical release record and longer-term design context; their old phase numbers are not a new implementation queue.

## Verified state and limits of this review

- **Workspace:** `D:/AI_Research/ReproBisect/` contains this handoff and publication drafts; it is not itself a Git repository. The Git checkout is `D:/AI_Research/ReproBisect/ReproBisect/`.
- **Local source:** clean `phase21-rc1` at short commit `368e97e`, with package `1.0.0-rc.1`. It lacks the upstream 1.1 `doctor`, detected-init, and output-discovery modules and the qualified dependency lock. Do not implement 1.2 against this old tree or infer missing upstream features from it.
- **Upstream refs, read-only check on 2026-10-01:** `main` remains `323714c875a63d304487a885a76290d01ff2422f`; `develop/1.1.0` remains `ee015fa22a261baddb3af2dad75e81783a55f339`, tree `2d221fc0e7adc87b4d23fd9212a2ca48c6f80fc9`; the only advertised tag is `v1.0.0` at `d96018d2959e285c8b1a70a4659d1badff26700a`. Verified with `git ls-remote --heads --tags origin`; no fetch, checkout, branch, or tag was changed.
- **Published release:** the live latest-release API still reports non-draft, non-prerelease `v1.0.0`. No public 1.1 release was observed.[1]
- **Qualification:** the API recheck confirms final-tree Release Qualification run `35350058321` and CI run `35350058382` completed successfully on `ee015fa...`; all six enumerated release jobs report success. This verifies recorded workflow state, not a new test execution or a reinspection of every historical log or asset.[2][3]
- **Source baseline:** pinned upstream package metadata says `1.1.0`, Rust edition 2024, MSRV 1.85, and `publish = false`.[4] Pinned upstream source inspection confirms `src/doctor.rs`, `src/init.rs`, and `src/output_discovery.rs` exist; the frozen evidence versions remain CheckReport 14, BuildRun/BuildFailure 10, FixReport 12, comparison 4.
- **Review coverage:** handoff/publication documents, nested source architecture, tests/fixture/corpus/workflow boundaries, selected pinned upstream 1.1 modules, and primary model/protocol documentation. No Rust/container qualification or model inference was run during this documentation-only review. Historical checksums and local `/mnt/data/...` artifacts below remain historical records unless explicitly reverified here.

**Next implementation prerequisite:** deliberately establish a separate development worktree based on the verified 1.1 commit, preserving the old checkout and the qualified release branch. Recheck refs and dirty/staged work first; do not reset or silently switch this checkout. Publication remains a separate owner-authorized operation. The header of upstream `docs/implementation-status.md` still uses release-candidate wording; successful final-tree workflow records are more recent qualification evidence, not permission to publish.

## Review findings and revised priorities

| Priority | Finding | Plan change |
|---|---|---|
| P0 | The local checkout and handoff describe different source generations. | Establish exact implementation base before development; preserve historical release identifiers. |
| P0 | Security/resource controls were scheduled after the CI Action and AI layer that need them. | Move run budgets, cancellation, disclosure controls, and untrusted-CI boundaries before those consumers. |
| P0 | Bounded logs are not secret-free logs. The current security document explicitly warns that builds can print secrets. | Keep raw evidence local by default; publish only an explicit, previewable derived summary. No default raw-log upload. |
| P1 | CI mode was an output sketch rather than an automation contract. | Specify deterministic status versus job policy, pure JSON stdout, versioned envelope, partial coverage, cancellation, escaped summaries, and pinned installation. |
| P1 | The model roadmap conflated typed decisions with generation and delayed all useful experimentation until 1.5. | Separate a small ranker trial from later config/patch generation. Start shadow evaluation after deterministic scheduling and corpus gates. |
| P1 | Generic AI benchmarks do not establish better causal debugging. | Measure time/builds to a **confirmed** finding and false attribution against static and rule-based schedules; retain negative controls. |
| P1 | Agent consumption is a plausible integration need, not grounds for a new autonomous debugger. | Stabilize the CLI/evidence contract first; optionally wrap read-only reports in MCP without adding execution authority. |
| P2 | Regression generalization has a valuable hypothesis but a costly initial 20–30-case commitment. | Use a small feasibility gate first; retain 20–30 unrelated cases as a later promotion gate, not a prerequisite for learning anything. |
| P2 | Cross-platform/package-manager breadth can outrun actual support. | Linux CI first; qualify arm64 separately. Document WSL2 with a Linux checkout before promising native Windows execution. |

The contemporary workflows to design for are: a maintainer diagnosing a local/release mismatch; a CI owner enforcing a declared policy; and an agent consuming evidence without inventing a verdict. These are proposed target users, not claims of measured demand. Validate them through external projects and record setup time, failures, and whether the finding was useful.

**Near-term product statement:** a local-first causal verification tool that humans, CI, and coding agents can call. Deterministic evidence remains the product; models may reduce investigation effort.

## What “Jev/Llaya” means in this plan

The user's spelling **“Llaya” is retained here as the original reference**. Research found a strong likely match: **Laya**, published by Convai Innovations (`convaiinnovations/laya`, source repository `NandhaKishorM/laya`). This is an interpretation, not a verified alias; confirm if another project was intended.[10][11]

Jev's official documentation explicitly says it is not a code-writing or chat model. It takes state plus typed `choice`, `score`, and `noul` questions.[5] Laya exposes the same broad decision pattern through local open weights; its card declares Apache-2.0.[10] Neither should be installed as ReproBisect's “root-cause oracle.”

| Candidate | Appropriate experimental role | Limit / decision |
|---|---|---|
| Deterministic rules; optional small supervised classifier | Default baseline: prioritize known dimensions from exact artifact/config features. | Cheapest operational baseline; keep if learned models add no measured value. |
| **Laya** | First local typed-ranker experiment over a short list of admissible interventions; later ambiguous triage. | Separate optional runtime; explicit checkpoint installation; domain evaluation/calibration required. No implied code-generation capability.[10][11] |
| **Jev** | Opt-in hosted comparator using the same bounded evidence view and candidate set. | Official API uses `/v1/systemone`; pin a versioned model rather than `jev-latest`. Current docs list `jev-1.13.0`. Data leaves the device; pricing/retention must be rechecked before activation.[7][8] |
| **Kev** | Optional open-model comparator if Laya's task quality is insufficient and hardware permits. | Its repository describes a self-hostable decision-model family and compatible endpoint. Do not adopt a training/deployment stack solely for this integration.[12] |
| **AnyJev** | Optional comparator when an appropriate local LLM/runtime already exists. | Its repository describes logit/hidden-state decision readouts and calibration; benchmark actual runtime and option-order sensitivity rather than assume API equivalence.[13] |
| Generative local/hosted LLM | Later free-form hypothesis, config, explanation, and patch proposals. | Separate capability and safety gate; generated config/patches are untrusted proposals, never direct canonical writes. |

### Model-specific cautions that change the design

- **Do not equate type safety with correctness.** A well-formed categorical answer can still be wrong. Jev documents adversarial-state susceptibility, weak numeric reasoning, and unreliable structural equivalences across separately asked questions.[9] All arithmetic, hashes, policy checks, and evidence predicates stay in code.
- **Do not map model confidence to causal confidence.** TypeSafe says Choice/Score confidence is derived from its probability distribution and Noul has no such field.[6] Laya documents a different confidence calculation.[11] Preserve provider-specific raw fields and a separately versioned local calibration record; never copy them into `Diagnosis.confidence`.
- **Laya is not automatically ready zero-shot.** Its own card reports base performance below the majority-class baseline on its typed-decisions benchmark, overconfidence, and a currently unhelpful `action.act_probability` signal. The advertised stronger typed-decisions result comes from the specialized checkpoint, not a demonstrated build-debugging model.[10] Do not use `act_probability` as an authorization gate.
- **Context is a design constraint.** Laya's card describes 512-token English and 1,024-token default multilingual/typed-decisions contexts, with an explicit larger multilingual setting. Options consume part of the same budget.[10] Feed small deterministic feature views, record truncation, refuse ambiguous/overflowed candidate encodings, and test long logs. Do not feed entire repositories or silently truncate decisive evidence.
- **Benchmark claims are not comparable product results.** Published Laya/Jev latency/accuracy tables include different prompts, sample sizes, hardware, and third-party Jev measurements.[10] This review ran neither model. No speedup, dollar saving, or reduction in build count is claimed for ReproBisect.
- **Reproducible research inputs:** observed Laya source revision `6d942c92081fbc139e736bbd9ac0023223c29b7f`, model repository revision `55cf4c4ebb4ebe31b2550e8bdf3bd21b99753851`; Kev source `90512f1c517d977741f2104470a40635408236c9`; AnyJev source `10d5db91dda38dbde74c6abc1c075ce6463723d1`. These identify inspected documentation, not approved or tested runtime dependencies. Resolve and record checkpoint, tokenizer, runtime, calibration, dtype, and license together at evaluation time.

### Recommended first experiment

Ask only: **“Among these already-admissible interventions, which should be tried next?”** Give the model concise artifact deltas, build-system features, completed outcomes, and explicit candidate IDs. Include abstention. The deterministic scheduler owns eligibility, remaining coverage, budgets, and confirmation.

Start in **shadow mode**: run the normal schedule, record model suggestions separately, and compare them after outcomes are known. A later gated mode may reorder eligible candidates; it may not drop candidates, shorten confirmation, override a baseline failure, or hide incomplete coverage. A timeout or invalid answer falls back to the deterministic schedule without a network fallback in local mode.

Keep typed decision records in a new, versioned advisory sidecar linked by evidence/plan digests. A cached prediction is not a cached experiment and never substitutes for a fresh repeated build. Local inference should finish before the measured build window or use isolated resources; otherwise it can itself perturb CPU/memory scheduling.

---

# Part I — Project handoff context

## 1. What ReproBisect is

ReproBisect is a Rust CLI for **causal debugging of non-reproducible builds**.

The motivating problem is:

> The same source code is built twice and produces different artifacts. Which environmental input actually caused the difference?

Tools such as `diffoscope` are excellent at answering **what changed in the artifact**. Tools such as `reprotest` can test whether a build remains reproducible while the environment varies. ReproBisect's distinguishing goal is to answer a stronger experimental question:

> Which tested environmental intervention repeatedly changes the output, and does reverting that intervention restore the baseline?

The core conceptual flow is:

```text
same source
   │
   ├── baseline build
   ├── baseline build          ← establish baseline stability
   │
   ├── controlled intervention A
   ├── repeat intervention A
   │
   ├── controlled intervention B
   │
   └── revert to baseline      ← confirm the effect disappears
```

A variable is not treated as strongly causal merely because:

- its value appears inside the artifact;
- the artifact changed at the same time;
- an LLM says it is likely;
- a structural diff suggests it.

The strongest diagnoses come from **intervention + repetition + reversal/reversion evidence**.

This principle is the project's main technical identity and should be protected as the codebase evolves.

### 1.1 Current scope vs. possible long-term scope

**Current product scope remains unchanged:** ReproBisect is a causal debugger for non-reproducible builds.

A broader strategic hypothesis has now been identified:

> The underlying intervention/repetition/reversal machinery may generalize from environmental build causes to **software regressions**, where the intervention is a commit, patch, hunk, dependency, configuration change, or other controlled software change and the oracle is a repeatable failing behavior.

This should **not** be interpreted as a decision to become a generic debugger or generic AI coding agent.

The broad debugging landscape already contains mature categories for:

- interactive runtime debugging and breakpoints;
- time-travel / record-replay debugging;
- static analysis and security scanning;
- production telemetry and incident/root-cause analysis;
- general coding agents that inspect code, propose patches, and run tests.

ReproBisect should not compete by duplicating those categories.

The potentially distinctive expansion is narrower:

> **Experimental causal debugging for software regressions: identify a minimal tested change whose removal eliminates a failure and whose introduction reproduces it, with repetition and reversal evidence.**

The current build-reproducibility engine is valuable even if this generalization never ships. The broader direction should be pursued only if a prototype demonstrates real value on unrelated regressions.

### 1.2 Candidate generalized abstraction

The architecture can be viewed more generally as:

```text
Subject
   ↓
Intervention
   ↓
Runner
   ↓
Oracle
   ↓
Causal Engine
   ↓
Evidence
```

The current build mode is one specialization:

```text
Subject       = build
Intervention  = environment/toolchain/dependency/build-input change
Runner        = isolated OCI build
Oracle        = artifact equality / structured artifact comparison
```

A future regression mode could be another specialization:

```text
Subject       = codebase / revision pair
Intervention  = commit / patch / hunk / dependency / configuration change
Runner        = isolated build + test execution
Oracle        = deterministic pass/fail or other declared behavior
```

Later domains such as performance regressions or input minimization may fit the same abstraction, but they should not be added until the simpler regression case is validated.

---

## 2. Project philosophy and invariants

The following principles should be treated as architecture-level invariants unless there is a compelling reason to change them.

### 2.1 Evidence before explanation

ReproBisect should make deterministic observations first and explanations second.

The intended order is:

```text
run experiment
→ capture environment/provenance
→ hash/analyze artifact
→ compare against baseline
→ repeat
→ revert/confirm
→ assign diagnosis/confidence
→ explain/remediate
```

Do not invert this so that a heuristic or model chooses the conclusion and the experiment merely decorates it.

### 2.2 LLMs, if added, are advisory

The intended LLM architecture is:

```text
LLM proposes hypothesis
        ↓
ReproBisect converts it into an admissible controlled experiment
        ↓
deterministic runner executes it
        ↓
artifact comparison measures the effect
        ↓
repetition + reversal confirm/refute it
        ↓
LLM may explain the verified result
```

LLMs may eventually help with:

- configuration generation;
- experiment ranking;
- unknown-cause hypothesis generation;
- diagnosis explanation;
- fix suggestions;
- interpreting build files and logs.

LLMs must **not** directly set deterministic report fields such as “root cause = X, confidence = high” without the existing evidence machinery.

### 2.3 Offline/non-LLM operation must remain first-class

ReproBisect must continue to work without:

- OpenAI;
- Anthropic;
- Gemini;
- an internet connection for model inference;
- any LLM at all.

A future assistant/provider layer should be optional and disabled by default.

### 2.4 The user's checkout should not be silently mutated

Experiments and fix verification should use fresh/temporary workspaces or copies where appropriate. The existing design already leans this way.

### 2.5 Persisted evidence compatibility matters

Evidence schemas are explicitly versioned and compatibility-tested. Treat schema changes as public-format changes, not ordinary internal refactors.

### 2.6 Reproducibility tooling must avoid hidden confounders

Future caching, acceleration, parallelism, LLM assistance, or CI integrations must not silently introduce state that invalidates experiments.

### 2.7 Generalization must preserve the causal contract

If ReproBisect expands beyond non-reproducible builds, the expansion must reuse the same epistemic standard rather than merely reuse the brand name.

For a future code-regression diagnosis, a strong claim should require evidence such as:

- a stable known-good behavior;
- a stable known-bad behavior;
- a controlled candidate change;
- repeated application/removal experiments where practical;
- reversal/reversion confirmation;
- explicit handling of interaction effects;
- persisted evidence showing exactly what was changed and what oracle was measured.

A model saying that a line “looks buggy” is not a ReproBisect causal diagnosis.

---

## 3. Current live GitHub state

This section preserves the detailed 2026-09-18 release record. The October review above rechecks current refs and workflow conclusions and distinguishes them from the older local checkout.

### Repository

- Repository: `trisanu-das/ReproBisect`
- Visibility: **public**
- Default branch: `main`
- Active 1.1 development/release branch: `develop/1.1.0`
- Package name: `reprobisect`
- Published package/release identity: `1.0.0`
- Final package version on `develop/1.1.0`: `1.1.0`
- Rust edition: `2024`
- Declared `rust-version`: `1.85`
- Repository qualification toolchain: Rust `1.85.1`
- `publish = false` remains in `Cargo.toml`; ReproBisect is not configured for crates.io publication.

### Published `main` / `v1.0.0` line

The public stable release remains `v1.0.0`. The previously recorded `main` snapshot was:

```text
commit: 323714c875a63d304487a885a76290d01ff2422f
tree:   54c5caa9d5c650273899283911b877698162a135
message: Revise README
```

The immutable `v1.0.0` tag remains:

```text
v1.0.0
→ d96018d2959e285c8b1a70a4659d1badff26700a

qualified release tree:
c3da9d600556cf419b4e256a23df6a82b5a7c5cf
```

`release-1.0.0` remains the frozen release branch for that published source. Do not rewrite or retag `v1.0.0`.

### Active/final 1.1 source tree

The active 1.1 branch is:

```text
branch:
develop/1.1.0

head:
ee015fa22a261baddb3af2dad75e81783a55f339

tree:
2d221fc0e7adc87b4d23fd9212a2ca48c6f80fc9

package version:
1.1.0
```

This is a **fully qualified final 1.1.0 source tree**, not yet a published release.

The final-tree normal CI run is:

```text
run ID:
35350058382

result:
success
```

The final-tree full Release Qualification run is:

```text
run ID:
35350058321

result:
success
```

That release workflow passed source policy, Rust `1.85.1`, stable Rust, Docker synthetic fixtures, Podman synthetic fixtures, the full pinned real-world corpus under both Docker and Podman, release-mode compilation, binary/package version equality, deterministic Linux packaging, and artifact upload.

### 1.1 release artifact produced by qualification

The final qualification produced:

```text
reprobisect-1.1.0-linux-x86_64
```

GitHub Actions artifact ID:

```text
10548948434
```

The workflow also retained:

```text
release-source-qualification   artifact ID 10549537108
release-real-world-docker      artifact ID 10549098186
release-real-world-podman      artifact ID 10549262818
```

These are qualification artifacts, not a GitHub Release. No `v1.1.0` tag has been created.

### Important branch rule right now

Until `v1.1.0` is intentionally published:

- treat `main` / `v1.0.0` as the public stable line;
- treat `develop/1.1.0` at `ee015fa...` as the qualified 1.1 release source;
- do not imply that 1.1 is already public;
- do not recreate or move `v1.0.0`;
- do not create `v1.1.0` unless publication is explicitly intended.

For ordinary feature development after 1.1 publication, return to a normal development branch rather than continuing release work on a frozen release identity.

## 4. GitHub Release state

A GitHub Release exists:

```text
tag:        v1.0.0
name:       ReproBisect v1.0.0
published:  2026-09-13
prerelease: false
draft:      false
```

Release URL:

https://github.com/trisanu-das/ReproBisect/releases/tag/v1.0.0

The release currently presents itself formally as the first stable release. `v1.1.0` is not yet published; the qualified 1.1 source exists only on `develop/1.1.0` until an explicit future publication step.

In public messaging, the recommended positioning remains conservative:

> **ReproBisect is an experimentally validated / qualified experimental causal debugger for non-reproducible builds.**

That better matches the breadth of validation: the core is strongly tested, but ecosystem breadth and external-user experience are still developing.

Do not mutate the contents of the existing `v1.0.0` release to make future fixes. Fixes should become `1.0.1`, `1.1.0`, etc.

---

## 5. `v1.0.0` release assets and checksums

The main published Linux artifact is:

```text
reprobisect-1.0.0-x86_64-unknown-linux-gnu.tar.gz
```

SHA-256:

```text
bd2430ad000a2521e0230bcdef6217d4d662b5a489fabab47745a375b2cf6c6e
```

The archive layout has been verified as:

```text
reprobisect-1.0.0-x86_64-unknown-linux-gnu/
reprobisect-1.0.0-x86_64-unknown-linux-gnu/LICENSE
reprobisect-1.0.0-x86_64-unknown-linux-gnu/README.md
reprobisect-1.0.0-x86_64-unknown-linux-gnu/SECURITY.md
reprobisect-1.0.0-x86_64-unknown-linux-gnu/reprobisect
```

The release also includes:

```text
reprobisect-1.0.0-ALL-SHA256SUMS.txt
```

Published release asset digest for that manifest:

```text
sha256:bf6b2287c0ab7a848cea2ccf50810493622209f6ae7dcf8f4422572ac3f3048a
```

The binary bundle was built deterministically with normalized tar metadata.

---

## 6. Local release/evidence artifacts produced during qualification

During release preparation, the following local directory was assembled:

```text
/mnt/data/reprobisect-release/1.0.0/
```

It contained:

```text
reprobisect-1.0.0-x86_64-unknown-linux-gnu.tar.gz
reprobisect-1.0.0-linux-x86_64.zip
reprobisect-1.0.0-linux-SHA256SUMS.txt
reprobisect-1.0.0-ALL-SHA256SUMS.txt
reprobisect-1.0.0-FREEZE.txt
evidence/release-source-qualification.json
evidence/real-world-docker.json
evidence/real-world-podman.json
```

A deterministic evidence bundle was also created:

```text
/mnt/data/reprobisect-release/reprobisect-1.0.0-release-evidence.tar.gz
```

SHA-256:

```text
ec41edb58c4d4a4eec48833100b6add0560be403f91c99c9ee7305cf3cd5304a
```

These paths are session/local-environment artifacts, not repository paths. A future maintainer should not assume they exist outside the original workspace; the important immutable records are the Git tag, GitHub Release, workflow runs, and checksums.

---

## 7. The authoritative stable qualification

The exact source promoted to `v1.0.0` was:

```text
commit: d96018d2959e285c8b1a70a4659d1badff26700a
tree:   c3da9d600556cf419b4e256a23df6a82b5a7c5cf
```

Authoritative stable Release Qualification workflow:

```text
run ID: 34758627742
```

The qualification completed successfully.

The green qualification matrix included:

- source/release policy checks;
- Rust `1.85.1` `cargo check --locked --all-targets`;
- Rust `1.85.1` `cargo test --locked --all-targets`;
- current stable Rust locked check/test;
- Docker synthetic fixture suite;
- Podman synthetic fixture suite;
- full seven-case real-world corpus under Docker;
- full seven-case real-world corpus under Podman;
- locked release build;
- exact binary version check: `reprobisect 1.0.0`;
- deterministic Linux release bundle;
- artifact upload.

Relevant job IDs recorded during qualification:

```text
source-policy:  103727329233
Rust 1.85.1:    103727348362
stable Rust:    103727348400
Podman:         103727483625
Docker:         103727483637
package-linux:  103728431598
```

This workflow is the main evidence that the released source passed the full intended release gate.

---

## 8. Qualified GitHub Actions artifacts

The release qualification produced these recorded workflow artifacts:

### Linux release artifact

```text
name: reprobisect-1.0.0-linux-x86_64
artifact ID: 10318805041
artifact ZIP SHA-256:
7c9bc45da92f3afa4776a3decafdca946a8f7a44706e3f7969e3e8143bfaf119
```

### Source qualification

```text
name: release-source-qualification
artifact ID: 10318575652
SHA-256:
9cb7e8d7dee285b00a2c136cbeebe395206307de91b263877209607012ad9506
```

### Podman real-world evidence

```text
name: release-real-world-podman
artifact ID: 10318431312
SHA-256:
1923c0f8aba89a4bc353c8f5ea294f49e11811505ac8070a9acc1de0a90b25ef
```

### Docker real-world evidence

```text
name: release-real-world-docker
artifact ID: 10317644098
SHA-256:
df9412cbc3e53a16c4b6fb4d51619371a0f3656ba47a7cff9a4f9a4eea26c1d0
```

The downloaded/reverified deterministic inner Linux tarball had the release checksum already listed above.

---

## 8A. Qualified `1.1.0` source tree and release-readiness history

The 1.1 line went through an explicit RC-to-final qualification cycle.

### Qualified RC

The feature-complete RC tree reached:

```text
commit:
3630b52d6065c3f84d1f39f13ee725dce96d9d52

package:
1.1.0-rc.1
```

Its normal CI and full release qualification both passed. The RC release qualification run was:

```text
35349014578
```

It produced a candidate artifact named:

```text
reprobisect-1.1.0-rc.1-linux-x86_64
```

### Final source-version promotion

After the RC was fully qualified, the branch source version was promoted to `1.1.0` without changing the persisted evidence schema family.

Final qualified source:

```text
commit:
ee015fa22a261baddb3af2dad75e81783a55f339

tree:
2d221fc0e7adc87b4d23fd9212a2ca48c6f80fc9
```

Final qualification:

```text
normal CI:
35350058382
success

release qualification:
35350058321
success
```

The release workflow now derives the package/archive version from `Cargo.toml`, verifies that the built binary reports exactly that version, and—on a tag-triggered run—rejects a Git tag that does not equal `v<package-version>`.

### Publication state

No tag or GitHub Release was created during implementation.

The future publication sequence is intentionally separate:

1. fast-forward `main` to the qualified 1.1 source;
2. create and push annotated tag `v1.1.0`;
3. require the tag-triggered Release Qualification workflow to pass;
4. publish the GitHub Release using the deterministic Linux bundle and `SHA256SUMS`;
5. update stable-install README examples afterward in a separate commit;
6. never move or recreate the published release tag.

## 9. Cargo/toolchain policy

The public stable line and active qualified 1.1 source currently differ in release identity:

```text
published stable:
1.0.0

qualified develop/1.1.0 source:
1.1.0
```

Current package metadata on `develop/1.1.0`:

```toml
[package]
name = "reprobisect"
version = "1.1.0"
edition = "2024"
rust-version = "1.85"
description = "Causal debugger for non-reproducible builds"
license = "Apache-2.0"
publish = false
```

Repository toolchain qualification still uses:

```text
Rust 1.85.1
```

The project has a committed `Cargo.lock`, and the root package entry on the qualified 1.1 source agrees with version `1.1.0`.

Locked dependency resolution remains mandatory across project/release gates:

```bash
cargo check --locked --all-targets
cargo test --locked --all-targets
cargo build --locked
cargo build --locked --release
```

Do not casually remove `--locked` from CI/release validation.

### Release-version guards

For 1.1, the release identity is intentionally cross-checked in multiple places:

- `Cargo.toml`;
- root `Cargo.lock` package entry;
- `scripts/release-check.py`;
- `scripts/static-check.py`;
- built binary `--version`;
- release tag on tag-triggered qualification.

This redundancy is deliberate. The 1.0 promotion previously exposed a stale static guard; the 1.1 process explicitly updates and re-qualifies all of these surfaces.

### Historical 1.0 lock information

The recorded 1.0 RC lock artifact SHA-256 remains:

```text
ae3414b86e7c5a89b71fe4b7e57fff11fc3e6de2dcf953665042fba0dbd788cb
```

Historical RC lock Git blob:

```text
b0b26ccd29bf439e37470d82cf07424dce58a382
```

These are historical 1.0 identifiers, not the current 1.1 release identity.

## 10. Persisted evidence schema freeze

The 1.0 release deliberately froze the persisted evidence schema family. The qualified 1.1.0 source intentionally preserves the same schema versions and supported ranges.

Current/supported ranges:

```text
CheckReport
  current:   14
  supported: 5–14

Build evidence
  current:   10
  supported: 1–10

FixReport
  current:   12
  supported: 2–12

EnvironmentComparison
  current:   4
  supported: 1–4
```

These versions were part of the release policy and test matrix.

A successor should **not** casually bump these versions while refactoring.

When a schema change is necessary:

1. define whether it is backward-compatible;
2. add compatibility/migration behavior;
3. update schema matrix tests;
4. update documentation;
5. update release-policy checks if needed;
6. keep old evidence readable within the declared supported range.

Relevant code areas include:

```text
src/schema.rs
src/compat.rs
src/evidence.rs
src/model.rs
src/report.rs
tests/evidence-schema-matrix.json
```

---

## 11. CLI exit-code contract

The project currently uses:

```text
0 = success
1 = diagnostic / finding / inconclusive outcome
2 = Clap usage error
5 = operational/internal failure
```

This distinction is important for CI integrations.

An exit code of `1` can be a **valid ReproBisect result**, not a crashed process.

When wrapping the CLI in workflows, do not treat every nonzero value as an infrastructure failure.

---

## 12. Configuration model and important defaults

`Config` consists broadly of:

```rust
pub struct Config {
    pub build: BuildConfig,
    #[serde(default)]
    pub experiments: ExperimentsConfig,
}
```

A minimal valid configuration looks like:

```toml
[build]
runner = "docker"
image = "gcc:14"
command = ["make"]
outputs = ["build/app"]
```

The build image, command, and outputs are core inputs.

The current default experiment-dimension behavior is:

```text
network_access    false
source_path       false
build_path        true
source_date_epoch true
timezone          true
locale            true
hostname          true
source_mtime      false
cpu_count         false
umask             false
directory_order   false
```

Additional targeted experiment types include:

- build-image variants;
- toolchain-variable variants;
- dependency-file variants;
- user-declared environment-variable variants.

Documented toolchain bindings include:

```text
CC
CXX
LD
AR
RANLIB
RUSTC
```

Toolchain experiments record whether the selected executable was actually invoked before promoting the result to strong evidence.

---

## 13. Core source-code layout

The current `src/` top level contains:

```text
src/
  artifact/
  cli.rs
  compat.rs
  config.rs
  doctor.rs
  engine/
  environment.rs
  evidence.rs
  init.rs
  main.rs
  model.rs
  output_discovery.rs
  report.rs
  runner/
  schema.rs
  source.rs
```

### Major responsibilities

#### `src/cli.rs`

CLI surface, command dispatch, command options, format selection, exit behavior.

#### `src/config.rs`

Configuration parsing, defaults, experiment dimensions and user-configurable variants.

#### `src/doctor.rs`

1.1 readiness/preflight checks for project configuration and the selected Docker/Podman runtime. Produces explicit PASS/WARN/FAIL/SKIP-style information in text or JSON.

#### `src/init.rs`

1.1 deterministic project/build-system detection and editable starter-config generation, including confidence/ambiguity reporting.

#### `src/output_discovery.rs`

1.1 opt-in temporary-build output discovery used by `init --discover-outputs`. It snapshots changed/new files, filters likely intermediates, classifies/ranks candidate artifacts, and is explicitly resource-bounded.

#### `src/compat.rs`

Backward compatibility across evidence versions.

#### `src/model.rs`

Core data structures shared across engine/evidence/report layers.

#### `src/evidence.rs`

Evidence representation/handling.

#### `src/environment.rs`

Environment representation and controlled environment metadata.

#### `src/source.rs`

Source snapshot/provenance/workspace-related logic.

#### `src/report.rs`

Human/machine report construction and output.

#### `src/schema.rs`

Current persisted schema constants/definitions.

### Artifact layer

Current artifact submodules:

```text
src/artifact/
  archive.rs
  elf.rs
  generic.rs
  mod.rs
  semantic.rs
```

This layer detects/analyzes artifacts and extracts structural/semantic evidence.

### Causal engine

Current engine modules:

```text
src/engine/
  compare.rs
  control.rs
  ddmin.rs
  diagnosis.rs
  fix.rs
  interventions.rs
  mod.rs
  statistics.rs
```

This is the conceptual center of the project.

Roughly:

- `control.rs` — experiment/control orchestration;
- `interventions.rs` — controlled variable changes;
- `compare.rs` — comparison logic;
- `diagnosis.rs` — evidence-to-diagnosis logic;
- `fix.rs` — fix proposals/verification;
- `ddmin.rs` — minimization support;
- `statistics.rs` — statistical support for stochastic effects.

### Runner layer

```text
src/runner/
  mod.rs
  oci.rs
```

`oci.rs` is large and central. It implements Docker/Podman build execution and many environment-control mechanics.

Be careful when modifying runner semantics: release qualification found multiple real-world edge cases there, especially around ownership and shell/tool behavior.

---

## 14. Artifact analysis capabilities

ReproBisect can hash outputs and perform structural analysis for multiple artifact families.

The implemented/referenced coverage includes:

- generic files;
- ELF;
- TAR;
- ZIP;
- `ar`;
- gzip;
- JAR;
- Python wheel;
- DEB;
- OCI tar/layout-related archives;
- PE/COFF;
- Mach-O;
- WebAssembly.

The intended philosophy is:

> ReproBisect should identify the causal environmental dimension; a tool such as diffoscope can still be used for very deep byte-level forensics.

The roadmap later proposes deeper semantic analysis, not replacing generic hashing.

---

## 15. Causal dimensions and capabilities already implemented

The current system covers or has machinery for:

- build/workspace path;
- source path;
- `SOURCE_DATE_EPOCH`;
- timezone;
- locale;
- hostname;
- source mtimes;
- CPU count/parallelism;
- umask;
- directory ordering;
- network access;
- build image;
- toolchain executable variants;
- dependency-file variants;
- user-declared environment variables;
- interaction search;
- repeated control/intervention runs;
- reversion confirmation;
- stochastic behavior/statistical evidence;
- provenance capture;
- fix proposals/verification for selected known causes.

Do not assume every dimension is enabled by default; see the defaults section above.

---

## 16. Fix behavior

The `fix` flow is intended to be conservative.

Examples of fix classes covered during development include:

- compiler path remapping;
- fixed `SOURCE_DATE_EPOCH`;
- source/archive mtime normalization;
- umask/mode-related normalization.

A key principle is:

> Proposed fixes are verified through a build/experiment loop; ReproBisect should not silently rewrite a user's project and claim success.

The roadmap proposes expanding LLM-assisted fix *suggestions*, but deterministic verification should remain required.

---

## 17. Real-world release corpus

The release corpus contains seven pinned real-world cases.

### Smoke cases

#### 1. cJSON — build path

```text
project: cJSON
case: cjson-debug-build-path
pinned SHA:
acc76239bee01d8e9c858ae2cab296704e52d916
```

Expected relevant cause: build path.

#### 2. tinyxml2 — build path

```text
project: tinyxml2
case: tinyxml2-debug-build-path
pinned SHA:
321ea883b7190d4e85cae5512a12e5eaa8f8731f
```

Expected relevant cause: build path.

#### 3. zlib — negative control

```text
project: zlib
case: zlib-release-path-control
pinned SHA:
51b7f2abdade71cd9bb0e7a373ef2610ec6f9daf
```

This is intentionally a real reproducible negative control.

#### 4. byteorder — direct `rustc` build path

```text
project: byteorder
case: byteorder-rustc-build-path
pinned SHA:
ec068eefa042d494475db125c4b034bd8e9e34dd
```

Expected relevant cause: build path.

### Extended cases

#### 5. Python sampleproject wheel — umask

```text
case: sampleproject-wheel-umask
pinned SHA:
3b0207a3396b634e41af20e82e0c710194d5f5dd
```

Expected relevant cause: umask.

#### 6. OpenSBI pre-prefix-map

```text
case: opensbi-pre-prefix-map
pinned SHA:
66ab965e54017292d530033ffaae127389c45458
```

Expected relevant cause: build path.

#### 7. OpenSBI post-prefix-map

```text
case: opensbi-post-prefix-map
pinned SHA:
6d23a9c5707de7243c0b3423724c0e4f3e1ee40d
```

Important nuance: this case intentionally still expects `non_reproducible` / `build_path`.

Empirical qualification showed that the upstream prefix-map configuration did **not** eliminate every path-dependent piece of debug metadata. Do not “fix” this expectation merely because the case name says post-prefix-map.

### Corpus location

Repository paths include:

```text
corpus/real-world/cjson-debug-build-path/
corpus/real-world/tinyxml2-debug-build-path/
corpus/real-world/zlib-release-path-control/
corpus/real-world/byteorder-rustc-build-path/
corpus/real-world/sampleproject-wheel-umask/
corpus/real-world/opensbi-pre-prefix-map/
corpus/real-world/opensbi-post-prefix-map/
```

Each case uses pinned configuration/expectation metadata.

---

## 18. Important bugs discovered during release qualification

Several issues were found only after running genuine CI/container qualification. They are important history because they reveal where the system is fragile.

### 18.1 Rootful Docker + umask ownership

Problem:

- rootful Docker builds under umask `077` created bind-mounted output owned by root;
- host-side cleanup/analysis could then fail or behave incorrectly.

Fix:

- after build/dependency summary, Docker umask experiments `chown -R` source/meta output back to host UID:GID;
- file bytes/modes remain meaningful;
- Podman behavior remains separate.

The umask fixture then passed with:

```text
non_reproducible
cause = umask
```

and fix verification for `pin-umask` also passed.

This logic lives around `src/runner/oci.rs`.

### 18.2 Direct `rustc` fixture and login shell PATH reset

Pinned Rust image:

```text
rust@sha256:9a73a5088750b4c95158ab26629c854c3d6fc4b173cb7bc8079ad252d8ed7bfa
```

Problem:

- invoking `rustc` through `sh -lc` in the Debian-based image caused a login-shell PATH reset.

Fix:

- invoke `rustc` directly for the corpus case.

Historical commit recorded:

```text
de1ea2ab...
```

### 18.3 Make-based fixtures and Debian image

Problem:

- a Debian image used by fixtures did not contain `make`.

Fix:

- switch the relevant corpus build to a pinned GCC image:

```text
gcc@sha256:3ae7320d7dd41f446a48930e1edf4e7a41c7be5ae43a5ddbf017c53fe6495738
```

Historical commit:

```text
41718bac...
```

### 18.4 OpenSBI helper missing Python

Problem:

- OpenSBI's Kconfig/build helper needed Python.

Fix:

- add `python3` to the helper image.

Historical commit:

```text
83e7199c...
```

### 18.5 OpenSBI expectation correction

The post-prefix-map case remained build-path dependent.

This was treated as an empirical result rather than forcing the test to match an assumption.

That is a useful precedent:

> Corpus expectations should follow verified behavior, not the maintainer's hoped-for theory.

---

## 19. Original RC1 history

The first release candidate was:

```text
version: 1.0.0-rc.1
commit: 94d8b2f1a3c87265280d9efca835adfce035a2e3
tree:   f24875f600be815a4089cc2fb463c4f5297da84b
```

Original local source bundles:

```text
/mnt/data/reprobisect-release/phase21/reprobisect-phase21.zip
/mnt/data/reprobisect-release/phase21/reprobisect-phase21.tar.gz
```

Real CI exposed issues after this freeze, leading to the qualification fixes described above.

A clean locked RC qualification was later achieved at:

```text
commit: 102d8abc796b15a4558ecf4d7e307fc4d2f41b58
tree:   d0160c8422a9097fabb667dfca0ff9d9c72e7185
release qualification run:
34757368857
```

All release-policy/Rust/Docker/Podman/corpus/package gates were green there.

Stable `1.0.0` promotion was then identity/release work rather than a feature rewrite.

---

## 20. Stable `1.0.0` promotion history

The first stable candidate:

```text
commit: 10852c3d7c9904f7f316e74cd5d7a1b3625c2f60
tree:   63dfd6a47a8f3be62a81d6d0c6b5abacb14b0949
```

Files intentionally changed in promotion included:

```text
.github/workflows/release.yml
Cargo.lock
Cargo.toml
CHANGELOG.md
SECURITY.md
docs/release-qualification.md
scripts/release-check.py
tests/evidence-schema-matrix.json
```

Promotion changes included:

- crate/package version `1.0.0`;
- lockfile root version `1.0.0`;
- dependency graph otherwise frozen;
- `publish = false`;
- schema versions unchanged;
- stable changelog entry;
- stable security support wording;
- release workflow expecting exact binary version `reprobisect 1.0.0`;
- package filename `reprobisect-1.0.0-x86_64-unknown-linux-gnu.tar.gz`.

The first stable gate correctly found one stale static guard in:

```text
scripts/static-check.py
```

It still required `1.0.0-rc.1`.

That guard alone was updated to stable `1.0.0`; the runtime/schema checks were not weakened.

Final qualified stable candidate:

```text
commit: d96018d2959e285c8b1a70a4659d1badff26700a
tree:   c3da9d600556cf419b4e256a23df6a82b5a7c5cf
parent: 10852c3d7c9904f7f316e74cd5d7a1b3625c2f60
```

That is the commit tagged `v1.0.0`.

---

## 21. Independent external validation: ChatterBot

After release qualification, ReproBisect was tested on a separate repository from the same GitHub account:

```text
trisanu-das/ChatterBot
```

This was selected because it was a comparatively substantial unrelated project and had a real Python packaging path.

Pinned ChatterBot commit:

```text
d911b4ac46e122308a759ea6ec67b666136e3026
```

Real output artifact:

```text
dist/ChatterBot-1.1.0a7-py2.py3-none-any.whl
```

### Validation branch

The test workflow was added on an isolated ReproBisect branch:

```text
external-validation-chatterbot
```

Current tip:

```text
a8d1df341903401a3d91753494ef1a723804dea6
```

Message:

```text
Add positive ChatterBot SOURCE_DATE_EPOCH validation
```

The test workflow is intentionally not part of the `v1.0.0` release source.

### GitHub Actions run

Positive-control validation run:

```text
run ID:
34778345522
```

The job completed successfully.

### Negative control 1: build path

Result:

```text
status: reproducible
diagnoses: 0
```

Artifact hash:

```text
62dd07bc1df1a598ea4edb646cc71703513268218c3bef3c719bc1f26513bf3f
```

Changing the build path did not change the wheel.

### Negative control 2: source mtime

Result:

```text
status: reproducible
diagnoses: 0
```

The wheel remained unchanged.

### Positive control: `SOURCE_DATE_EPOCH`

Baseline epoch:

```text
946684800
```

Intervention epoch:

```text
1577836800
```

Baseline wheel hash:

```text
62dd07bc1df1a598ea4edb646cc71703513268218c3bef3c719bc1f26513bf3f
```

Variant wheel hash:

```text
8943e7fa9c2bacec52bfe9b4cec36a7c60e0efa7ecb0dc9f0727005830eaef42
```

ReproBisect report:

```text
status: non_reproducible
diagnosis title:
Build output depends on build timestamp input

confidence:
high
```

The wheel contained 65 members, and the structural evidence localized the observed change to member timestamps.

Reversion restored the original baseline result.

This external test is important because it demonstrated **both sides**:

1. ReproBisect did not manufacture a cause for two inert interventions.
2. It did detect and confirm a real timestamp-input effect on an unrelated project.

### Evidence artifact

A local copy of the workflow evidence was downloaded as:

```text
/mnt/data/chatterbot-reprobisect-validation-positive.zip
```

It contains:

```text
chatterbot-build-path.json
chatterbot-source-mtime.json
chatterbot-source-date-epoch.json
```

Again, local `/mnt/data` paths are not durable project storage; retain/port the evidence into the repository if it is considered important for long-term regression testing.

---

## 22. Current README state

The README was revised after `v1.0.0` and extended again during 1.1 development.

It now leads with:

> ReproBisect is a causal debugger for non-reproducible builds.

The README includes:

- the baseline/intervention/reversion explanation;
- a minimal `.reprobisect.toml`;
- `reprobisect check .`;
- comparison against diffoscope/reprotest/manual rebuilds;
- installation;
- Docker/Podman guidance;
- default experiment dimensions;
- targeted toolchain/dependency experiments;
- `reprobisect doctor`;
- deterministic build-system-aware `reprobisect init`;
- init confidence/ambiguity behavior;
- opt-in `reprobisect init . --discover-outputs`;
- the diagnosis-first 1.1 text report;
- the precise Rust policy: declared MSRV `1.85`, repository qualification toolchain `1.85.1`.

The earlier README nuance about saying “requires Rust 1.85.1 or newer” has therefore been resolved in the 1.1 branch.

### Stable-install wording before publication

Because `v1.1.0` has not yet been published, the 1.1 branch README intentionally distinguishes:

- public/prebuilt stable instructions: still `1.0.0`;
- source built from `develop/1.1.0`: reports `1.1.0`.

After `v1.1.0` is actually published, update the stable/prebuilt installation examples in a separate post-release documentation commit. Do not change the release tag to incorporate that documentation-only follow-up.

## 23. Current CI/release workflows

The repository contains:

```text
.github/workflows/ci.yml
.github/workflows/real-world.yml
.github/workflows/release.yml
```

### `ci.yml`

Normal development checks remain the fast/default validation path.

The final qualified 1.1.0 source at `ee015fa...` passed:

```text
run ID:
35350058382

result:
success
```

The run covered:

- source policy;
- Rust `1.85.1`;
- stable Rust;
- Docker fixture suite;
- Podman suite;
- real-world smoke corpus.

### `real-world.yml`

Runs pinned corpus validation outside the standard smoke path.

### `release.yml`

The 1.1 release workflow is the stronger qualification/package matrix.

It now:

- runs for `develop/1.1.0`;
- accepts `v1.1.*` tags;
- can be manually dispatched;
- uses a same-ref concurrency group and cancels superseded qualification runs;
- runs source-policy qualification;
- runs Rust `1.85.1` and current stable check/test;
- runs Docker and Podman synthetic qualification;
- runs the full pinned real-world corpus under both runtimes;
- performs a locked release build;
- derives the package/archive version from `Cargo.toml`;
- verifies the binary reports exactly that version;
- on tag builds, verifies `GITHUB_REF_NAME == v<package-version>`;
- creates a normalized deterministic Linux tar/gzip bundle;
- uploads source/corpus/release artifacts.

Final-tree Release Qualification:

```text
run ID:
35350058321

result:
success
```

Locked Cargo operations remain intentional in all of these workflows.

## 24. Release source vs current `main`

Keep three identities distinct:

```text
published v1.0.0 source
d96018d2959e285c8b1a70a4659d1badff26700a
        │
        ├── post-v1.0 README work on main
        │
        └── 1.1 development line
                 ↓
develop/1.1.0
ee015fa22a261baddb3af2dad75e81783a55f339
package 1.1.0
fully qualified, not yet tagged/published
```

The original `v1.0.0` release qualification validates only its tagged source.

The final 1.1.0 source has its **own** complete qualification:

```text
CI:
35350058382

Release Qualification:
35350058321
```

When publishing 1.1, preserve the exact qualified source identity. Prefer a fast-forward of `main` rather than manufacturing an unnecessary merge commit between qualification and tagging.

Until publication, do not describe `ee015fa...` as an already released GitHub version.

## 25. Publication/positioning state

The project has already been published publicly on GitHub.

The recommended maturity description is:

```text
research prototype
    ↓
qualified experimental tool   ← current practical stage
    ↓
production-hardened tool
```

Do not oversell it as universally solving reproducibility.

A good public description is:

> ReproBisect is an experimental causal debugger for non-reproducible builds. It systematically perturbs controlled build inputs, compares artifacts structurally, and confirms candidate causes by intervention and reversal.

A useful tester call-to-action is:

> If you have a real build ReproBisect cannot explain, please provide the project or a minimized reproducer.

The recommended development loop is:

```text
release
→ observe real failures
→ turn failures into fixtures
→ fix
→ regression test
→ release again
```

---

## 26. Outreach already discussed

Recommended public channels, in approximate order:

1. GitHub repository / Releases / Discussions;
2. Reproducible Builds community (`rb-general`, community channels);
3. Show HN;
4. LinkedIn;
5. targeted Reddit communities;
6. Mastodon / reproducible-builds community social channels.

GitHub and LinkedIn v1.1 announcement drafts, plus a very short v1.1 publication guide, have now been prepared as separate Markdown artifacts in conversation. They are not part of the repository source unless deliberately copied there.

The key outreach framing is:

> “I built an experimental tool that tries to identify which environmental input causally makes a build non-reproducible. I’m looking for real builds that it cannot explain.”

---

## 27. Original implementation-plan status

The long original implementation process was organized into 21 phases.

**All 21 are complete.**

There is no pending “Phase 22.”

A successor should not continue numbering historical phases. New work should use the post-release roadmap (`1.0.x`, `1.1`, etc.) included in Part II of this document.

---

## 28. Important engineering guardrails for the next maintainer/agent

### 28.1 Do not weaken deterministic evidence to make tests pass

If a real project behaves differently from an expectation:

1. investigate the project and intervention;
2. verify whether the assumption was wrong;
3. update the expectation only if evidence supports it.

The OpenSBI post-prefix-map case is the precedent.

### 28.2 Do not silently turn shell behavior into a confounder

The byteorder `sh -lc`/PATH incident demonstrates that wrapper-shell semantics can invalidate experiments.

Prefer the narrowest reliable invocation.

### 28.3 Treat container ownership as part of runner correctness

Docker vs Podman differences in UID/GID, rootfulness, bind mounts, and umask can affect both experiments and cleanup.

### 28.4 Keep build outputs and metadata distinguishable

An environmental intervention should not accidentally alter the harness in ways indistinguishable from the target project.

### 28.5 Keep negative controls

Every new class of intervention should have cases where it:

- definitely changes output;
- definitely does not change output.

Avoid a corpus consisting only of positive detections.

### 28.6 Preserve machine-readable evidence

Human output can evolve rapidly. Persisted JSON should evolve carefully.

### 28.7 Performance optimizations must preserve experimental independence

Caching and parallelism are future priorities, but only when their state is explicitly controlled.

### 28.8 Fix verification remains experimental evidence

A suggested fix is not “verified” until the post-fix build reproduces according to the defined criteria.

---

## 29. Immediate next actions recommended before deeper feature work

The original 1.1 adoption tasks are no longer “next actions”; they are implemented and qualified.

### Completed for 1.1

- `reprobisect doctor` core readiness checks;
- build-system-aware `reprobisect init`;
- deterministic project detection for Cargo, Go, npm/pnpm, Maven, Gradle, Bazel, Meson, CMake, Autotools, Python packaging, and Make;
- explicit init inference confidence and ambiguity handling;
- opt-in bounded output discovery via `reprobisect init . --discover-outputs`;
- Docker/Podman selection through `init --runner`;
- diagnosis-first human-readable reports with compact tested-variable summaries;
- 1.1 release/version/workflow hardening;
- complete RC and final-source qualification.

### Immediate decision point

There are now two legitimate paths:

1. **Publish 1.1.0 later** from the already-qualified source, following the explicit tag-triggered qualification process; or
2. **Continue implementation toward 1.2** while keeping the qualified 1.1 release source identifiable and unmodified for future publication.

If the owner chooses publication, do not add features between final qualification and tagging. Fast-forward `main`, tag `v1.1.0`, require the tag workflow to pass, then publish the generated bundle.

### Next product-development target after 1.1

The roadmap's next major implementation layer is still `1.2`:

- CI-native operation;
- predictable automation output;
- official GitHub Action;
- GitHub Step Summary integration;
- incremental distribution improvements.

Do **not** skip this layer merely because a broader debugger vision is now conceivable. CI-native operation improves the current build product and also provides infrastructure a future regression-debugging mode would need.

### New strategic research track after the CI-native layer

After the 1.2 adoption layer is usable, prototype a deliberately narrow command such as:

```bash
reprobisect regress   --good <known-good-revision>   --bad <known-bad-revision>   --test "./test.sh"
```

The prototype should accept only a **repeatable executable oracle** at first. Its purpose is to test whether ReproBisect can provide materially stronger evidence than commit-level bisection alone.

Minimum experimental loop:

1. confirm the good revision repeatedly passes;
2. confirm the bad revision repeatedly fails;
3. locate a candidate commit/change range;
4. minimize changed files/hunks where feasible;
5. remove/revert a candidate from the bad tree and retest;
6. transplant/apply the candidate to the good tree where semantically valid and retest;
7. repeat successful interventions;
8. search small interactions when no single change explains the failure;
9. emit a causal evidence report rather than merely a suspect line.

Do not rename/reposition the whole project around this prototype until it succeeds on a substantial unrelated regression corpus.

### Still valuable in parallel

The external-project campaign remains important. Target unrelated real builds, classify failures consistently, and convert correctness defects into regression cases. The ChatterBot validation remains useful evidence and should eventually be retained in durable project documentation or a maintained external corpus rather than living only as an isolated validation branch/artifact.

## 30. Suggested failure taxonomy for external testing

Use consistent labels when collecting real-world failures:

```text
CONFIG_DISCOVERY_FAILURE
BUILD_UNSUPPORTED
RUNNER_FAILURE
EXPERIMENT_NOT_EFFECTIVE
FALSE_POSITIVE
FALSE_NEGATIVE
ARTIFACT_ANALYSIS_GAP
PERFORMANCE_FAILURE
PROVENANCE_GAP
INTERACTION_REQUIRED
USER_ERROR_NOT_ACTIONABLE
```

This makes it possible to prioritize work without needing invasive telemetry.

False positives should be treated as particularly high priority because they damage the core causal claim.

---

## 31. What “production-ready” should eventually mean

Do not equate feature count with production readiness.

A reasonable eventual bar includes:

- a substantial external corpus across ecosystems;
- no known high-severity false-positive class;
- stable evidence/config compatibility policy;
- predictable CI integration;
- documented security model;
- bounded resource behavior;
- strong error diagnostics;
- reproducible releases;
- acceptable performance on large builds;
- multiple external maintainers independently finding previously unknown causes.

The strongest success criterion is:

> External maintainers use ReproBisect to discover causes that were not pre-designed into ReproBisect's tests.

---

## 32. Collaboration style that worked during development

For future coding agents/LLMs working with the project owner:

- prefer concrete execution over repeatedly asking what to do next;
- when a task can be safely completed autonomously, complete it;
- test changes against real behavior, not just static reasoning;
- report discovered conceptual bugs early;
- avoid inventing new project phases;
- preserve the user's preference for concise progress updates but thorough technical deliverables;
- do not declare success solely because a workflow exits green—inspect generated evidence when the semantic result matters;
- when external validation is requested, use unrelated real projects rather than only synthetic fixtures.

---

## 33. Quick reference: immutable identifiers

### Stable release

```text
version: 1.0.0
tag: v1.0.0
release commit:
d96018d2959e285c8b1a70a4659d1badff26700a

release tree:
c3da9d600556cf419b4e256a23df6a82b5a7c5cf

release qualification run:
34758627742

Linux tarball SHA-256:
bd2430ad000a2521e0230bcdef6217d4d662b5a489fabab47745a375b2cf6c6e
```

### Current main

```text
commit:
323714c875a63d304487a885a76290d01ff2422f

tree:
54c5caa9d5c650273899283911b877698162a135

latest verified main CI run:
34840830255
result:
success
```

### Qualified 1.1.0 source (not yet published)

```text
branch:
develop/1.1.0

commit:
ee015fa22a261baddb3af2dad75e81783a55f339

tree:
2d221fc0e7adc87b4d23fd9212a2ca48c6f80fc9

package:
1.1.0

normal CI:
35350058382
success

release qualification:
35350058321
success

qualified Linux workflow artifact:
reprobisect-1.1.0-linux-x86_64

artifact ID:
10548948434

publication state:
NO v1.1.0 tag or GitHub Release yet
```

### External ChatterBot validation

```text
branch:
external-validation-chatterbot

branch tip:
a8d1df341903401a3d91753494ef1a723804dea6

workflow run:
34778345522

ChatterBot commit:
d911b4ac46e122308a759ea6ec67b666136e3026

baseline wheel SHA-256:
62dd07bc1df1a598ea4edb646cc71703513268218c3bef3c719bc1f26513bf3f

SOURCE_DATE_EPOCH variant SHA-256:
8943e7fa9c2bacec52bfe9b4cec36a7c60e0efa7ecb0dc9f0727005830eaef42
```

### Evidence schemas

```text
CheckReport:             current 14, supports 5–14
Build evidence:          current 10, supports 1–10
FixReport:               current 12, supports 2–12
EnvironmentComparison:   current 4,  supports 1–4
```

---

## 34. Handoff instruction to the next human/LLM

If you are continuing the project from this document:

1. clone/open `trisanu-das/ReproBisect`;
2. understand the branch/release distinction before editing anything:
   - `v1.0.0` is the current published stable release;
   - `develop/1.1.0` at `ee015fa22a261baddb3af2dad75e81783a55f339` is a fully qualified final `1.1.0` source tree that has not yet been published;
3. never rewrite or retag `v1.0.0`;
4. do not mutate the qualified 1.1 release identity if the immediate goal is publication;
5. if publishing 1.1, fast-forward `main`, tag `v1.1.0`, require the tag-triggered full release workflow to pass, and only then create the GitHub Release;
6. if instead continuing feature development, the next roadmap layer is `1.2` CI-native/distribution work;
7. after the 1.2 layer is usable, the approved broader-scope experiment is the regression-debugging research track—not a generic AI-debugger rewrite;
8. run locked Rust tests before and after code changes;
9. run synthetic fixtures for causal-engine/runner changes;
10. run relevant real-world corpus cases for intervention/OCI/artifact changes;
11. preserve persisted evidence compatibility unless a deliberate compatibility/migration change is being made;
12. preserve bounded behavior in output discovery: file scan cap, bounded stdout/stderr tails, candidate cap, and 600-second build timeout;
13. prioritize real external failures over speculative breadth;
14. keep LLMs outside deterministic evidence authority.

The remainder retains the detailed post-publication roadmap. October revisions make Parts II 8/15/25/26 and Part III authoritative for execution order; historical completion notes do not mean these features exist in the old local checkout.


# Part II — Improvement roadmap

# ReproBisect Improvement Roadmap

**Status:** Post-`v1.0.0` roadmap; the planned `1.1` first-run/adoption slice is implemented and fully qualified, with publication pending. A gated software-regression causal-debugging research track has been added without changing the current product scope.  
**Purpose:** Define a staged path from a qualified experimental causal debugger to a broadly useful, production-hardened reproducible-build diagnostics platform, while testing—without prematurely pivoting—whether the same causal machinery can generalize to evidence-backed software regression diagnosis.  
**Planning horizon:** Acceptance-gated milestones; old month ranges in retained design sections are historical estimates, not scheduling commitments. See Part III.

> **Core principle:** ReproBisect should remain an evidence-first causal debugger. Heuristics, automation, and LLMs may propose what to test, but only controlled experiments, repetition, structural artifact comparison, and reversal should promote a hypothesis into a diagnosis.

---

## 1. Executive summary

ReproBisect has reached an important threshold: it is no longer merely a prototype.

The `v1.0.0` line already demonstrates a coherent end-to-end model:

1. establish a reproducible baseline;
2. perturb one or more controlled build inputs;
3. rebuild in an isolated OCI environment;
4. compare declared outputs;
5. localize structural artifact differences where possible;
6. repeat interventions;
7. revert to the baseline;
8. distinguish evidence-backed causal findings from correlation or inconclusive behavior.

The project has also passed synthetic fixtures, a pinned real-world corpus under Docker and Podman, and an independent external-project validation where ReproBisect correctly rejected inert hypotheses and identified a real `SOURCE_DATE_EPOCH` dependency.

### Current roadmap progress — historical 2026-09-20 summary; rechecked 2026-10-01

The `1.1` adoption/first-run objectives have now been implemented and qualified:

- deterministic project/build-system detection in `init`;
- explicit inference confidence and ambiguity reporting;
- bounded opt-in output discovery;
- `doctor` readiness checks in text/JSON;
- diagnosis-first terminal reports;
- release-candidate/final release workflow hardening.

The qualified final source is `develop/1.1.0` at `ee015fa22a261baddb3af2dad75e81783a55f339`. It is not yet tagged or published.

Accordingly, the next planned implementation layer is `1.2` CI-native operation/distribution, while external-corpus expansion continues in parallel.

A separate strategic question is now worth testing:

> **Can ReproBisect generalize from “which environment change caused this artifact difference?” to “which software change causally caused this regression?”**

The answer is not assumed to be yes. The generic debugging/AI coding space is already crowded, and ReproBisect does **not** need a broader scope to justify the current product. The research opportunity is specifically the interventional layer: controlled code changes, repeated good/bad oracles, reversal, minimal causal sets, and interaction search.

The next phase should therefore not be “add as many features as possible.”

It should be:

> **Turn the existing causal engine into something real maintainers can install, configure, trust, run repeatedly, and extend.**

The roadmap is ordered by leverage:

1. observe real users and enlarge the validation corpus;
2. reduce setup friction and improve first-run UX;
3. make results easier to understand and consume in CI;
4. prototype causal regression debugging on known-good/known-bad revisions with a deterministic oracle;
5. improve performance and experiment selection for large builds;
6. deepen artifact-aware and interaction-aware causal diagnosis;
7. add LLM assistance around the deterministic core;
8. expand ecosystem/platform support;
9. continue advanced sandbox/platform work; baseline security, disclosure, resource bounds, and compatibility are prerequisites throughout, not last-stage additions.

The regression prototype is a **validation gate**, not an automatic expansion of product scope. If it cannot demonstrate stronger causal localization than ordinary revision bisection on real regressions, the build-focused roadmap should continue without the broader pivot.

The LLM layer should arrive only after the deterministic workflow is easy to operate. Its role should be to propose hypotheses, generate configuration, prioritize experiments, explain evidence, and suggest fixes—not to replace causal verification.

---

# 2. Product doctrine

Before discussing releases, it is useful to make the project's design doctrine explicit.

## 2.1 Causal validity

A diagnosis should mean:

> Under a controlled intervention performed by ReproBisect, changing a tested input repeatedly changed the declared observable outcome, and reverting the input restored the baseline behavior or otherwise satisfied the configured confirmation criteria.

For the current build product, the observable is normally artifact equality/structure. For a future regression mode, it may be a test result, crash predicate, assertion, exit status, or another explicitly declared oracle.

This is stronger than:

- “the path appears in the binary”;
- “these files differ”;
- “the model thinks timestamps are suspicious”;
- “the build log contains an environment variable.”

Those may be clues, but they are not causal conclusions.

## 2.2 Practical usability

A technically correct tool can still fail as a product if users must understand all internal experiment semantics before the first successful run.

The desired experience is eventually:

```bash
reprobisect init .
reprobisect doctor
reprobisect check .
```

with sensible project detection, diagnostics, and actionable output.

## 2.3 Auditability

A user should be able to answer:

- What was built?
- Under which environment?
- Which variable changed?
- How many baseline and intervention runs were performed?
- What changed in the artifact?
- Was the intervention repeated?
- Was it reversed?
- Why was the confidence level assigned?
- Which evidence is persisted?

The machine-readable evidence schema is therefore a product surface, not an implementation detail.

## 2.4 Bounded intelligence

ReproBisect may become increasingly intelligent, including through LLMs, but intelligence must remain bounded by the experimental model.

A heuristic or LLM may say:

> “`PYTHONHASHSEED` is a plausible next hypothesis.”

It should not be allowed to directly upgrade that into:

> “Root cause: `PYTHONHASHSEED`, confidence: high.”

The deterministic engine must still test the claim.

## 2.5 Scope discipline: do not become a generic debugger

A broad label such as “general debugger for codebases” is not itself a product strategy.

ReproBisect should **not** attempt to replace:

- interactive debuggers;
- record/replay or time-travel debuggers;
- static analyzers;
- production observability/RCA systems;
- generic coding agents.

Those tools solve different observation and interaction problems.

The expansion worth testing is narrower:

> **causal regression debugging through controlled software interventions.**

That keeps the project's differentiator intact: ReproBisect proves or refutes candidate causes experimentally rather than winning by breadth of code understanding.

## 2.6 Necessity and sufficiency are contextual evidence, not absolute claims

For a candidate software change `P`, a future regression mode may perform two complementary experiments:

```text
bad revision - P  → does the failure disappear?
good revision + P → does the failure appear?
```

These are powerful **tested-context** necessity/sufficiency signals.

They must not be overstated as universal mathematical necessity/sufficiency because:

- a patch may depend on surrounding context;
- multiple changes may be interchangeable causes;
- interactions may be required;
- cherry-picking a hunk may be semantically invalid;
- the oracle may be flaky or incomplete.

Reports should therefore say exactly which intervention was tested on which base revision and with which repeated outcome.

---

# 3. Release strategy

The following version numbers are planning labels, not promises. A release should happen only when its exit criteria are satisfied.

| Release line | Primary objective | Current status |
|---|---|---|
| `1.0.x` | harden the published experimental release | `v1.0.0` published; no separate 1.0.x patch release required so far |
| `1.1` | dramatically reduce setup and first-run friction | **implemented + fully qualified; `v1.1.0` publication pending** |
| `1.2` | safe, budgeted CI contracts, selective evidence export, pinned Action/distribution | **next planned product layer; safety gates first** |
| `1.3` | deterministic cost-aware scheduling; optional typed-decision shadow trial | future; model promotion separately gated |
| `1.4` | deepen artifact-aware diagnosis and causal minimization | future |
| `1.5` | optional generative config/explanation/fix assistance | future; distinct from the earlier typed-decision trial |
| `1.6` | broaden ecosystem adapters, packaging, and provider support | future |
| `2.0` candidate | cross-platform / remote / production-oriented architectural expansion | future / only if architectural pressure justifies it |

A useful rule is:

> Every minor release should be justifiable by a new category of real build that becomes materially easier to diagnose.

### Unversioned research track: software regression causal debugging

Do **not** assign this experiment a promised release number yet.

After the CI-native 1.2 layer, build a narrow prototype around:

```text
known-good revision
known-bad revision
deterministic executable oracle
```

Only after validation on unrelated real regressions should the project decide whether this becomes:

- a new command inside the 1.x line;
- a plugin/adapter over the causal engine;
- a larger 2.0 architectural direction;
- or an experiment that should be abandoned while keeping ReproBisect build-focused.

This avoids allowing an attractive vision to force a premature architectural rewrite.

---

# 4. Phase 0 — Immediate post-publication hardening

**Indicative window:** Weeks 0–4  
**Release target:** `1.0.1`, `1.0.2`, etc.  
**Theme:** Do not add ambitious functionality yet. Observe what breaks.

## 4.1 Goals

The purpose of the first post-publication phase is to convert early-user friction into regression tests and documentation.

Actively seek:

- build systems not represented in the current corpus;
- confusing setup errors;
- container-runtime incompatibilities;
- false positives;
- false negatives;
- unsupported artifact layouts;
- unexpectedly expensive experiment matrices;
- permission/ownership failures;
- ambiguous reports;
- problems caused by generated or dirty source trees.

## 4.2 Build a real external validation corpus

Create an `external-corpus/` or similarly isolated validation layer containing pinned test cases from unrelated projects.

Target at least:

- 3–5 C/C++ projects;
- 3 Python package projects;
- 2–3 Rust workspaces;
- 2 Go projects;
- 2 Java/Maven or Gradle projects;
- 2 JavaScript/TypeScript package builds;
- at least one Autotools project;
- at least one Meson project;
- at least one Bazel or similarly complex build.

The corpus should intentionally contain both **positive** and **negative** controls.

### Positive controls

Cases where a known environmental variable changes output:

- `SOURCE_DATE_EPOCH`;
- absolute path embedding;
- archive member timestamps;
- locale-sensitive generated text;
- toolchain variation;
- umask-sensitive packaging.

### Negative controls

Cases where a suspicious variable is changed but should not affect the artifact.

Negative controls are essential because a causal-debugging tool must prove that it can **avoid diagnoses**, not just produce them.

## 4.3 Establish a failure taxonomy

Every failed external run should be assigned a class such as:

- `CONFIG_DISCOVERY_FAILURE`
- `BUILD_UNSUPPORTED`
- `RUNNER_FAILURE`
- `EXPERIMENT_NOT_EFFECTIVE`
- `FALSE_POSITIVE`
- `FALSE_NEGATIVE`
- `ARTIFACT_ANALYSIS_GAP`
- `PERFORMANCE_FAILURE`
- `PROVENANCE_GAP`
- `INTERACTION_REQUIRED`
- `USER_ERROR_NOT_ACTIONABLE`

This taxonomy later becomes a practical planning tool without requiring invasive telemetry.

## 4.4 Improve error quality before adding features

Every operational error should ideally answer three things:

1. **What failed?**
2. **Why does ReproBisect think it failed?**
3. **What should the user try next?**

Instead of:

```text
OCI runner failed with exit code 125
```

prefer:

```text
Docker could not start the experiment container.

Possible causes:
  - Docker daemon is not running
  - current user cannot access the Docker socket
  - configured image could not be pulled

Check:
  docker info

Or switch to:
  runner = "podman"
```

## 4.5 Exit criteria

Phase 0 is complete when:

- at least 15–20 unrelated external projects have been attempted;
- every newly discovered correctness bug has a regression test;
- the most common runtime/configuration failures have actionable messages;
- no known release-blocking false-positive bug remains;
- installation and first-run documentation reflect real user experience.

---

# 5. Phase 1 — First-run UX and project discovery

**Indicative window:** Months 1–2  
**Release target:** `1.1`  
**Theme:** A user should not need to understand ReproBisect before successfully configuring it.

**Current status (2026-09-18): core 1.1 objective COMPLETE.**

Implemented on `develop/1.1.0`:

- deterministic detection for Cargo, Go, npm/pnpm, Maven, Gradle, Bazel, Meson, CMake, Autotools, Python packaging, and Make;
- editable image/command/output suggestions;
- explicit high/medium/low confidence;
- ambiguity reporting when multiple build markers compete;
- low-confidence fallback instead of pretending a weak guess is authoritative;
- `--runner docker|podman`;
- opt-in `--discover-outputs`.

The generic configuration model remains authoritative. The detailed design notes below are retained because they explain the intended semantics.

This was one of the highest-return product improvements after publication.

## 5.1 Evolve `reprobisect init` into a project detector

The generic configuration model should remain authoritative, but `init` should infer a starting configuration.

Potential detectors:

| File / signal | Suggested build family |
|---|---|
| `Cargo.toml` | Cargo |
| `pyproject.toml` | Python packaging |
| `setup.py` / `setup.cfg` | legacy Python packaging |
| `CMakeLists.txt` | CMake |
| `Makefile` | Make |
| `meson.build` | Meson |
| `go.mod` | Go |
| `package.json` | npm/pnpm/yarn |
| `pom.xml` | Maven |
| `build.gradle*` | Gradle |
| `WORKSPACE`, `MODULE.bazel` | Bazel |
| `configure.ac` | Autotools |

The detector should not silently assume correctness. It should emit something like:

```text
Detected project:
  build system: CMake
  confidence: high

Suggested build:
  cmake -S . -B build
  cmake --build build

Candidate outputs:
  build/bin/example

Suggested container:
  gcc:14

Configuration written:
  .reprobisect.toml

Review the generated file before running:
  reprobisect check .
```

## 5.2 Add explicit inference confidence

Inference output should distinguish:

- `high`: unambiguous manifest/build metadata;
- `medium`: reasonable convention-based guess;
- `low`: heuristic discovery requiring user review.

This later provides a clean foundation for LLM-assisted configuration.

## 5.3 Output discovery

Automatically finding final artifacts is difficult, but partial support would be valuable.

Possible strategy:

1. run a normal build in a temporary workspace;
2. snapshot changed/new files;
3. filter obvious intermediates;
4. identify executable/archive/package candidates;
5. present the user with a shortlist.

Example:

```text
Possible final artifacts detected:

[1] target/release/myapp       ELF executable
[2] target/release/libfoo.so   shared library
[3] target/release/build/...   likely intermediate
```

Do not automatically treat every modified file as an output.

**Implemented 1.1 behavior:** the probe copies the source to a temporary workspace, takes before/after snapshots, filters obvious intermediates, reuses artifact-type detection, ranks likely executable/archive/package outputs, prints a shortlist, and only lets **high-confidence** candidates replace static `build.outputs`.

Resource bounds now include:

- 100,000-file snapshot/traversal cap;
- at most 12 ranked candidates;
- 16 KiB retained tail per stdout/stderr stream;
- concurrent pipe draining to avoid deadlock/unbounded buffering;
- 600-second temporary-build timeout;
- named temporary container + watchdog kill on timeout.

A Docker end-to-end regression verifies discovery of a final ELF, rejection of a `.o` intermediate, and no build mutation in the original checkout.

---

# 6. Phase 2 — `reprobisect doctor` and environment readiness

**Indicative window:** Months 1–2, overlapping Phase 1  
**Release target:** `1.1`  
**Theme:** Detect environmental failures before an expensive experiment starts.

**Current status (2026-09-18): core 1.1 objective COMPLETE.**

Implemented:

- config validation;
- selected Docker/Podman executable check;
- runtime/daemon reachability;
- configured-image readiness;
- missing local image reported as **warning**, not hard failure;
- explicit text readiness states;
- JSON doctor report.

The broader wishlist below—such as disk-space probes, deeper host capability checks, and support-bundle generation—remains future enhancement work rather than a blocker for the completed 1.1 doctor slice.

## 6.1 Add `reprobisect doctor`

The command should inspect:

### Host prerequisites

- Docker availability;
- Podman availability;
- daemon/socket access;
- architecture;
- Git availability where relevant;
- filesystem support;
- available disk space;
- container pull/run ability.

### Project configuration

- valid TOML;
- build runner recognized;
- configured image available or pullable;
- build command syntactically valid;
- declared output paths plausible;
- source tree readable;
- output directory writable.

### Experiment compatibility

For example:

- hostname experiments may require specific container behavior;
- network experiments require runner support;
- tracing may require Linux features/tools;
- user namespace limitations may affect ownership behavior.

## 6.2 Machine-readable doctor output

```bash
reprobisect doctor --format json
```

This becomes useful for CI bootstrapping and support requests.

## 6.3 Support bundles

A safe support command could eventually generate:

```bash
reprobisect doctor --support-bundle support.tar.gz
```

containing bounded, redacted diagnostic information:

- ReproBisect version;
- OS/architecture;
- runner versions;
- sanitized config;
- failed readiness checks;
- no source code;
- no secrets;
- no raw environment dump.

---

# 7. Phase 3 — Human-readable diagnostics redesign

**Indicative window:** Months 2–3  
**Release target:** `1.1` / early `1.2`  
**Theme:** The default terminal output should expose the causal reasoning clearly.

**Current status (2026-09-18): COMPLETE for the 1.1 default text report.**

The default human view now leads with result, promoted cause, confidence, affected artifacts, baseline/variant/reversion hashes when available, evidence, remediation, and a compact effect/no-effect list of tested variables. Full experiment/provenance/trace/build-log detail remains available with `-v`.

The JSON evidence remains detailed, while most users can understand the primary result without opening it.

## 7.1 Diagnosis summary

Desired structure:

```text
ReproBisect diagnosis

Artifact:
  dist/example.whl

Status:
  NON-REPRODUCIBLE

Causal variable:
  SOURCE_DATE_EPOCH

Confidence:
  HIGH

Observed experiment:
  baseline #1    62dd07bc...
  baseline #2    62dd07bc...
  intervention   8943e7fa...
  intervention   8943e7fa...
  reverted       62dd07bc...

Structural difference:
  65 ZIP member timestamps changed
  member payload hashes remained unchanged

Suggested remediation:
  export a fixed SOURCE_DATE_EPOCH during packaging
```

## 7.2 “Tested but inert” reporting

Users should see not only the cause found, but also important hypotheses ruled out:

```text
Tested with no observed effect:
  build_path
  timezone
  locale
  hostname
```

That is one of ReproBisect's differentiators.

## 7.3 Explainability boundaries

Reports should distinguish:

- **observed fact**;
- **structural interpretation**;
- **causal diagnosis**;
- **heuristic suggestion**;
- later, **LLM-generated explanation**.

These must never be blended into one undifferentiated confidence score.

---

# 8. Phase 4 — CI-native operation and distribution

**Release target:** `1.2`; acceptance-gated, not calendar-gated.  
**Theme:** A safe unattended run with an honest result and a shareable, bounded summary.

## 8.1 CI contract before CI presentation

Proposed `--ci` behavior must be defined in tests before public syntax is frozen:

- no interactive prompts, color, or progress on machine-readable stdout;
- one valid versioned JSON envelope on stdout; bounded progress/diagnostics on stderr;
- source/config/binary/run identities, operation kind, report location/digest, diagnostic status, completed/planned/skipped coverage, and completion reason;
- strict separation of the historical diagnostic exit contract (`0/1/2/5`) from a caller's fail-on policy; default exits remain backward-compatible;
- findings, incomplete/budget-limited runs, cancellation, and infrastructure errors remain distinct. Failing to obtain evidence can never produce a green reproducibility claim;
- planned first-finding mode reports incomplete search even after a confirmed finding; an all-causes label never claims unmodeled global coverage;
- existing create-only evidence remains authoritative. Prefer a new orchestration envelope/sidecar over casual changes to frozen historical report fields.

## 8.2 GitHub Action: minimal, pinned, least privilege

Implement in this repository first (`action.yml` plus a bounded wrapper), not a speculative second action repository. Proposed inputs: explicit config, runtime, binary version/checksum, time/build budgets, fail-on policy, and **upload-evidence: false**. Proposed outputs: diagnostic status, completion reason, report path/digest, and summary path. Final syntax is frozen only after end-to-end tests.

Requirements:

- consumers pin a reviewed full action commit; the wrapper installs a specific qualified binary and verifies its checksum, architecture, and version;
- no floating latest binary or pipe-to-shell installer; no arbitrary user input interpolated into shell source;
- read-only repository permissions by default; no model credentials or secrets in fork/untrusted build jobs;
- never check out untrusted PR code under privileged `pull_request_target` or `workflow_run`; separate build and trusted publication jobs;
- fail closed on missing/malformed reports, timeout, checksum mismatch, or unsupported policy; preserve the actual diagnostic status when mapping CI failure policy;
- collect bounded safe summaries on failure/cancellation where possible, with explicit retention limits and no implicit raw-log/source upload.

GitHub's current security guidance specifically warns about privileged untrusted checkout, secret-redaction limitations, and mutable action dependencies.[14] Pin all third-party actions in the implementation; this plan deliberately provides no fictitious deployable action SHA.

## 8.3 Summaries and disclosure

Render deterministic evidence first, with links to locally retained details and an explicit incomplete/unsupported-experiment summary. Escape workflow-command sequences, terminal controls, Markdown/HTML injection, and untrusted paths/text. Do not let artifact strings generate Actions commands or active markup.

Use P03's allowlisted derived view for both summaries and optional uploads. Redaction is not proof of anonymity. Preserve the exact original evidence separately; a sanitized digest is not the original evidence digest. A selected share bundle is not yet a source-minimized reproducer.

## 8.4 Distribution boundaries

Start with qualified Linux x86_64 and the Action. Add arm64 only with native architecture/runtime/corpus qualification; cross-compilation or emulator success alone is insufficient. Verify downloaded package checksum plus recorded source/binary identity; add attestations as supply-chain provenance, not proof that user builds are reproducible.

`publish = false` currently prevents a normal crates.io-based install path.[4] Treat `cargo-binstall`, Homebrew, AUR, and container distribution as demand-led follow-ups with explicit packaging decisions. A containerized orchestrator must not casually require mounting the host Docker socket.

Document WSL2 as a Linux workflow, using a Linux-filesystem checkout and a separately qualified OCI setup. Do not claim native Windows support from Rust cross-compilation or successful PE artifact parsing. Host, runner, target architecture, filesystem, and artifact format are separate axes.

---

# 8A. Parallel research track — Causal debugging of software regressions

**Timing:** Start only after the CI-native 1.2 layer is usable.  
**Release target:** none initially; this is a validation track, not a promised feature release.  
**Theme:** Test whether ReproBisect's causal model generalizes beyond build-environment nondeterminism without becoming a generic debugger.

## 8A.1 Initial problem statement

Start with the narrowest useful regression question:

> A known-good revision repeatedly passes an executable oracle, and a known-bad revision repeatedly fails it. Which code/dependency/configuration change is causally responsible for that regression?

Example prototype interface:

```bash
reprobisect regress   --good v2.3.0   --bad HEAD   --test "./test.sh"
```

The first prototype should require the user to provide the oracle. Do not begin by trying to infer arbitrary correctness from source code.

## 8A.2 Why this is different from ordinary bisection

Revision bisection can identify the first commit in history where an oracle changes. That is useful, but a commit is not necessarily the minimal cause.

A single commit may contain:

- refactoring;
- formatting;
- multiple features;
- dependency changes;
- generated files;
- configuration changes;
- the actual fault.

ReproBisect should attempt to go from:

```text
culprit commit
```

to stronger experimental evidence:

```text
candidate change
   ↓
remove from bad revision
   ↓
failure disappears repeatedly
   ↓
introduce into good revision where valid
   ↓
failure appears repeatedly
   ↓
reversal / repetition / interaction checks
   ↓
minimal causal evidence
```

## 8A.3 Proposed generalized engine interfaces

Do not begin by rewriting every module. First identify a narrow internal abstraction boundary:

```text
Subject
Intervention
Runner
Oracle
Causal Engine
Evidence
```

The current build implementation maps naturally:

```text
Subject       = source/build
Intervention  = environment/build-input change
Runner        = OCI build runner
Oracle        = artifact comparison
```

Regression debugging would map to:

```text
Subject       = revision/codebase
Intervention  = commit/patch/hunk/dependency/config change
Runner        = build + test runner
Oracle        = declared deterministic behavior
```

The experiment should reveal whether this abstraction is real or merely aesthetically attractive. Do not force the current codebase into a framework before the prototype proves the need.

## 8A.4 Prototype algorithm

A useful first implementation sequence:

1. **Control qualification**
   - run the good revision multiple times;
   - run the bad revision multiple times;
   - reject or explicitly model a flaky oracle before causal search.

2. **Revision localization**
   - use Git history search/bisection to identify a candidate commit or compact range.

3. **Change decomposition**
   - enumerate changed files;
   - decompose into patches/hunks where mechanically safe;
   - classify dependency/config/build-script changes separately from source changes.

4. **Bad-tree removal experiment**
   - revert/remove a candidate from the bad revision;
   - rebuild/retest;
   - repeat when the failure disappears.

5. **Good-tree introduction experiment**
   - apply/transplant the candidate onto the good revision where semantically valid;
   - rebuild/retest;
   - repeat when the failure appears.

6. **Minimization**
   - reuse/adapt `ddmin` to shrink a multi-hunk or multi-file causal set.

7. **Interaction search**
   - when no single candidate explains the failure, test small combinations;
   - report a minimal tested causal set instead of arbitrarily blaming one change.

8. **Evidence report**
   - persist exact base revisions;
   - exact applied/reverted patch identities;
   - oracle command and outcomes;
   - repetition counts;
   - failed/invalid transplant attempts;
   - interaction/minimization trace;
   - contextual confidence rationale.

## 8A.5 Contextual necessity/sufficiency language

Suppose candidate patch `P` is under test.

Evidence may look like:

```text
bad - P  → PASS 8/8
good + P → FAIL 8/8
```

A strong report may say:

> In the tested revision contexts, removing `P` from the bad tree eliminated the failure and introducing `P` into the good tree reproduced it.

Do **not** simplify that to an absolute statement such as “P is universally necessary and sufficient.”

## 8A.6 Interaction cases are strategically important

This track becomes more distinctive when the cause is not a single commit.

Examples:

```text
code change A alone        → pass
dependency update B alone  → pass
A + B                      → fail
```

or:

```text
code change X
+
Python/runtime version Y
+
locale/configuration Z
→ fail
```

The existing ReproBisect heritage in environmental intervention and interaction search can become an advantage here.

A future regression mode should therefore allow code interventions and existing environment interventions to participate in the same causal set where practical.

## 8A.7 Validation corpus

Do not reposition the project based on synthetic examples.

First use **3–5 unrelated real regressions** across at least two ecosystems for feasibility, including a nontrivial interaction or invalid transplant. Stop early if the oracle/patch model is not useful. Before promoting the abstraction into a supported product, retain the stronger target of **20–30 unrelated real regressions** across multiple ecosystems; this target is not a statistical proof of generality.

For each case record:

- repository and exact good/bad revisions;
- oracle;
- ordinary bisection result;
- ReproBisect minimal tested cause;
- whether bad-tree removal succeeded;
- whether good-tree introduction succeeded;
- whether interactions were required;
- runtime/cost;
- invalid patch/transplant cases;
- false-positive/false-negative outcome;
- whether the result gave materially more information than commit-level bisection.

Include negative controls where suspicious changed code is experimentally inert.

## 8A.8 Go/no-go criteria

The broader regression-debugging direction is worth promoting only if all safety/correctness gates and a predeclared practical-value gate pass; a majority vote over desirable properties is insufficient:

- stable handling of deterministic pass/fail oracles;
- materially finer localization than commit-level bisection on real regressions;
- low false-positive rate;
- useful evidence when transplants are impossible or partially applicable;
- successful multi-change/interaction cases;
- acceptable experiment cost;
- reports that users can audit without trusting an LLM;
- no weakening of build-reproducibility behavior.

If these criteria are not met, keep ReproBisect focused on reproducible-build diagnosis and retain any reusable infrastructure without forcing a product pivot.

## 8A.9 What explicitly remains out of scope for this track

Do not begin with:

- arbitrary memory-corruption debugging;
- interactive breakpoint/state inspection;
- full record/replay debugging;
- distributed production incident RCA;
- autonomous issue-to-patch agents;
- “read this entire codebase and find every bug”;
- unrestricted natural-language correctness oracles.

Those are different products.

---

# 9. Phase 5 — Performance architecture for large builds

**Indicative window:** Months 3–6  
**Release target:** `1.3`  
**Theme:** Reduce the number and cost of builds without corrupting experimental validity.

For large projects, runtime will likely become the dominant usability problem.

If a single build takes 20 minutes and ReproBisect performs 20–40 builds, a technically correct diagnosis may still be unusable.

## 9.1 Model build cost explicitly

The scheduler should understand:

- expected build duration;
- experiment priority;
- required repetition count;
- whether an intervention can be grouped;
- whether a previous result makes another experiment unnecessary.

## 9.2 Safe caching

Caching must be introduced conservatively because caches can themselves be confounders.

### Reusable inputs only after identity and isolation checks

- downloaded container layers;
- immutable source snapshot;
- ReproBisect's own analysis results;
- content-addressed static dependencies verified not to vary across conditions.

Content addressing alone does not establish safety: permissions, materialization paths, timestamps, resolver/network behavior, and mutable write layers can still change observations. Never cache repeated build observations as if they were fresh trials. Analysis caches include parser version and all bounds/options. Isolate or disable build/dependency caches unless their lifecycle is declared and held constant; keep warm/cold validation separate.

### Potentially confounding caches

- compiler caches;
- Cargo incremental state;
- Maven/Gradle caches;
- npm caches;
- generated configure output;
- build directories.

ReproBisect should never silently reuse a cache that could invalidate the intervention.

## 9.3 Parallelize independent experiments

If resources allow:

```text
timezone ─────────┐
locale ───────────┼─ parallel workers
hostname ─────────┤
build path ───────┘
```

while preserving per-experiment isolation and avoiding shared-resource interference. Keep CPU/stochastic experiments serial until interference is measured; pin resource policy and record co-scheduled work. A faster scheduler that changes the outcome distribution is a changed experiment, not a transparent optimization.

## 9.4 Early stopping

If an intervention produces a clear, repeated, reversible effect, ReproBisect may stop lower-priority unrelated experiments depending on policy.

Potential modes:

```text
--search first-cause
--search all-causes
```

`first-cause` prioritizes speed.

`all-causes` continues after one cause is found.

---

# 10. Phase 6 — Adaptive causal search

**Indicative window:** Months 4–7  
**Release target:** `1.3` / `1.4`  
**Theme:** Stop treating all experiments as equally likely.

This is where ReproBisect can become substantially more sophisticated without requiring an LLM.

## 10.1 Artifact-informed prioritization

### ELF with absolute paths in DWARF

Prioritize:

1. build path;
2. source path;
3. compiler path-remapping flags.

### ZIP/Wheel where only timestamps differ

Prioritize:

1. `SOURCE_DATE_EPOCH`;
2. source mtime;
3. timezone.

### Archive where member ordering changes

Prioritize:

1. directory enumeration;
2. parallelism;
3. hash/random iteration order;
4. archive tool behavior.

The key rule is:

> Structural evidence may reorder experiments, but it must not replace them.

## 10.2 Build-system-informed prioritization

Examples:

- Python packaging → timestamp/hash-seed/archive experiments;
- C/C++ debug builds → path/toolchain experiments;
- Go → VCS metadata/toolchain/time experiments;
- Java archives → timestamp/order/locale/toolchain experiments.

## 10.3 Score-based scheduler

A future scheduler could maintain hypothesis priority:

```text
candidate            supporting evidence   next action
------------------------------------------------------
SOURCE_DATE_EPOCH    high                  test now
build_path           medium                test next
locale               low                   defer
hostname             very low              defer
```

This score should guide execution order, not diagnosis confidence.

---

# 11. Phase 7 — Better interaction discovery and minimization

**Indicative window:** Months 5–8  
**Release target:** `1.4`  
**Theme:** Efficiently diagnose causes that do not appear in isolation.

Some failures are interaction effects:

```text
variable A alone → no change
variable B alone → no change
A + B            → artifact changes
```

## 11.1 Improve interaction search

Potential improvements:

- grouped interventions;
- hierarchical partitioning;
- ddmin-based minimization;
- interaction budget controls;
- prioritization using artifact/build-system evidence;
- repeatability checks at each minimization stage.

## 11.2 Define minimal causal sets carefully

A result should be phrased as:

> “Within the tested variable set, `{A, B}` is a 1-minimal intervention set observed to change the artifact.”

Avoid:

> “A and B are the only causes.”

That would exceed the evidence.

## 11.3 Causal-set visualization

Example:

```text
                 changes artifact?
A                      no
B                      no
C                      no
A+B                    yes
A+C                    no
B+C                    no

Minimal observed set:
  {A, B}
```

---

# 12. Phase 8 — Richer artifact-aware diagnostics

**Indicative window:** Months 5–9  
**Release target:** `1.4`  
**Theme:** Move from “hash differs” to “this semantic field changed.”

## 12.1 ELF

Potential additions:

- DWARF path extraction;
- `DW_AT_comp_dir`;
- source-file references;
- GNU build-id analysis;
- `.comment`;
- symbol-table ordering;
- note sections;
- section-level hashing.

Example:

```text
Changed bytes are confined to:
  .debug_info
  .debug_line

Observed:
  /workspace/A/src/foo.c
    →
  /workspace/B/src/foo.c

Candidate dimension:
  build_path
```

## 12.2 ZIP / Wheel / JAR

Distinguish:

- payload changes;
- member timestamps;
- member ordering;
- compression metadata;
- permissions/external attributes;
- generated metadata files.

## 12.3 TAR / ar / DEB

Distinguish:

- member order;
- uid/gid;
- mode;
- mtime;
- archive-format headers;
- control/data payload changes.

## 12.4 PE, Mach-O, WebAssembly

Add similarly bounded structural summaries where real use cases justify them.

---

# 13. Phase 9 — Minimized reproducer export

**Indicative window:** Months 6–9  
**Release target:** `1.4` or `1.5`  
**Theme:** Turn a diagnosis into something maintainers can share.

A future command:

```bash
reprobisect export-reproducer report.json --output repro.tar.gz
```

could generate a bounded package containing:

- sanitized ReproBisect configuration;
- exact experiment dimension;
- baseline/intervention environment delta;
- build command;
- artifact hashes;
- relevant structural difference summary;
- optional minimal source subset where safe and feasible.

This would help with:

- bug reports;
- upstream maintainers;
- compiler/toolchain investigations;
- CI reproducibility reports.

Source minimization itself is a harder research problem and should remain optional.

---

# 14. Phase 10 — LLM-assisted ReproBisect

**Timing:** Generative assistance remains later; the separate typed-decision shadow trial can start after P06/M01 in Part III.  
**Release target:** `1.5` planning label for generation, not a restriction on research.  
**Theme:** Add intelligence around the experiment engine without allowing the model to manufacture evidence.

This can become one of the strongest later improvements, but it must be architected carefully.

## 14.1 Principle: LLMs propose; ReproBisect proves

The desired loop:

```text
build evidence
      ↓
LLM proposes hypothesis
      ↓
ReproBisect validates that the experiment is admissible
      ↓
controlled intervention
      ↓
rebuild
      ↓
artifact analysis
      ↓
repeat + reversal
      ↓
verified/refuted hypothesis
      ↓
LLM explains the verified result
```

The LLM never controls final evidence semantics.

## 14.2 LLM use case A — Configuration generation

The model can inspect:

- manifests;
- build scripts;
- CI files;
- project documentation;
- directory structure.

Then propose a configuration.

Example:

```toml
[build]
runner = "docker"
image = "python:3.12"
command = ["python", "-m", "build", "--wheel"]
outputs = ["dist/example-0.1.0-py3-none-any.whl"] # illustrative; resolve the actual exact path
```

The CLI should make the trust boundary explicit:

```text
AI-generated configuration.
Not yet validated.

Run:
  reprobisect doctor
```

A deterministic schema/policy validator must run first. Doctor is a readiness check, not a proof of command safety or full hermeticity. Any generated build command requires explicit approval of its exact config/source context before an isolated, budgeted smoke test. Resolve actual output paths using bounded discovery; the example filename is a placeholder, not an inferred output or a supported glob contract.

## 14.3 LLM use case B — Experiment prioritization

Given:

- artifact type;
- structural diff;
- build system;
- logs;
- previous failed hypotheses;

the model can rank likely next experiments.

Example:

```text
Suggested next hypotheses:

1. PYTHONHASHSEED
   Reason: generated metadata order differs across runs.

2. locale
   Reason: generated files contain locale-formatted text.

3. dependency resolver state
   Reason: lockfile absent and dependency versions differ.
```

These are **hypotheses**, not findings.

## 14.4 LLM use case C — Explanation

A command such as:

```bash
reprobisect explain report.json
```

could produce a concise interpretation.

The output should explicitly identify itself:

```text
AI-assisted explanation
-----------------------
The deterministic report established that ...
```

The underlying evidence must remain inspectable.

## 14.5 LLM use case D — Fix suggestions

Once ReproBisect has deterministic evidence that `build_path` is causal, the model can inspect the build system and suggest concrete remediations:

- GCC/Clang prefix-map flags;
- Rust remap flags;
- CMake path remapping;
- archive normalization;
- Python packaging timestamp normalization;
- deterministic traversal;
- fixed environment exports.

Any automated fix should still pass through ReproBisect's normal verification loop.

## 14.6 LLM use case E — Unknown-cause investigation

This is the most ambitious and valuable use.

Suppose all built-in dimensions are exhausted:

```text
No tested intervention explains the difference.
```

The model can inspect:

- structured artifact difference;
- build logs;
- provenance;
- manifest/build files;
- experiment history.

It might propose:

```text
1. PYTHONHASHSEED
2. git describe output
3. uname/kernel-derived version string
4. filesystem case sensitivity
5. code generator version
6. nondeterministic protobuf enumeration
```

ReproBisect then converts supported hypotheses into controlled experiments.

This makes the model a **hypothesis generator**, not an oracle.

---

# 15. Decision and generative provider architecture

Use capability boundaries, not a single trait that forces every model to generate text. Jev/Laya typed decision endpoints are not OpenAI chat-completions APIs.[5][8][11]

Proposed internal seams (not implemented):

```text
DecisionProvider.rank(DecisionRequest) -> AdvisoryRanking | Abstain
GenerativeProvider.propose_config(ProjectView) -> UntrustedConfigProposal
GenerativeProvider.explain(EvidenceView) -> CitedExplanation
GenerativeProvider.suggest_patch(DiagnosisView) -> UntrustedPatchProposal
```

The first implementation only needs `DecisionProvider`; do not scaffold unused generation providers. Reuse `src/engine/interventions.rs::plan_interventions` to enumerate admissible candidates and introduce `src/engine/scheduler.rs` for ordering. Keep `src/engine/diagnosis.rs`, exact comparison predicates, statistical tests, and fix-verification authority out of the provider interface.

### Decision request contract

- Versioned request with source/config/evidence digests, candidate-set digest, explicit opaque candidate IDs, question/template revision, deterministic features, truncation flags, and token/byte ceilings.
- Candidate IDs map to existing validated intervention objects in memory. The model cannot add an executable, path, environment binding, network privilege, image, or patch.
- One closed-set decision at a time, with an explicit abstention outcome. A candidate can be useful without being the only cause; a Choice distribution is not an exhaustive causal probability distribution.
- Bootstrap from stable baseline artifact metadata and deterministic build/config features. Do not pretend an intervention delta exists before the intervention ran. Later ranking may use completed deltas; benchmark records must exclude future outcomes.
- Model sees bounded derived evidence, not arbitrary binary bytes or full logs. Optional excerpts need provenance IDs, disclosure classification, and exact source spans.

### Decision response contract

Persist `schema_version`, request/candidate/evidence digests, provider/model revision, runtime/checkpoint/tokenizer/calibration identity when available, raw probability distribution, provider-native confidence fields, selected candidate, abstention/fallback reason, measured duration, usage, and egress mode. Unknown remote runtime details remain unknown, never guessed.

Validate before use: matching request and model identity; exact candidate membership; no duplicate IDs; finite, bounded probabilities; normalized distribution within a documented tolerance; input/output caps; required fields; and compatible schema. Reject unsupported fields that could be mistaken for instructions. A malformed response is advisory failure, not project failure.

Keep `model_score`, provider confidence, empirical calibration, and deterministic diagnosis confidence distinct in names, storage, and UI. Recalibration is required after changing model, tokenizer, prompt, option set, quantization/dtype, or context construction. Retain raw predictions for audits; never rewrite historical causal evidence to match a new model.

### Three modes and execution authority

1. **Off (default):** deterministic rules only, no model SDK/runtime/network calls.
2. **Local AI:** explicitly installed/pinned Laya or another qualified adapter. Prefer a bounded, explicitly configured subprocess protocol for the first Rust integration. If HTTP is used, bind loopback, authenticate, reject redirects and non-loopback resolution, cap payloads, and qualify the exact server; an OpenAI-style URL is not proof of locality.
3. **Hosted AI:** explicit Jev or other stage-specific provider consent, destination allowlist, bounded retries within a deadline, spend ceiling, no secrets in logs, and no automatic fallback from local mode.

An advisory record is not execution approval. Code validates the proposed action against the current source/config/plan hash and reserves remaining run allowance before dispatch. A changed source tree/config/candidate set invalidates the suggestion. Save model responses by the complete input/model/policy identity; never reuse them across users or projects merely because a short prompt matches.

### Shadow, replay, then controlled promotion

- **Shadow:** record predictions without changing schedule or outcomes. Run inference outside build measurement windows.
- **Replay evaluation:** measure candidate ranking against fully observed eligible outcomes, retaining unknown labels as unknown. This estimates ordering only, not unseen interaction outcomes or real wall-time savings.
- **Live paired trial:** run fixed/rule/model schedules under equivalent budgets, repeated on isolated runs, and measure time to experimentally confirmed findings. Include cold runtime startup and model loading.
- **Promotion:** opt-in reorder-only mode after M03's gate. No pruning, confirmation reduction, safety override, or positive reproducibility claim after incomplete coverage.

Generative hypothesis/config/fix assistance remains separate. It needs a source-bound proposal, review/approval, admissibility checks, and the existing temporary-checkout verification loop. Model prose remains a derived view linked to authoritative evidence.

---

# 16. LLM security and privacy model

This boundary must be implemented before the first model-backed feature or public evidence upload, not merely documented before a later release.

## 16.1 Default: off

No source code, logs, or evidence should be sent to an external model unless the user explicitly enables it.

## 16.2 Data preview

Before transmission, ReproBisect should be capable of showing something like:

```text
The following data will be sent to provider X:

  3 build files
  1,842 lines
  structural artifact summary
  redacted build log
  experiment history

Excluded:
  environment secrets
  raw network endpoints
  binary artifact contents
```

Non-interactive CI should require explicit configuration.

## 16.3 Redaction

Never blindly send:

- full environment variables;
- tokens;
- credentials;
- `.env` files;
- SSH material;
- package-registry credentials;
- arbitrary home-directory files.

## 16.4 Local-model mode

Local AI is the first optional model path, not a late privacy add-on. No implicit model downloads, telemetry, or hosted fallback. A missing checkpoint must leave the deterministic CLI usable. Hosted adapters require a separately approved bounded payload, destination, and spend ceiling. See the October review and Part III for Laya/Jev-specific controls.

---

# 17. Phase 11 — Ecosystem adapters

**Indicative window:** Months 7–12  
**Release target:** `1.6`  
**Theme:** Make common build systems feel native without forking the core architecture.

Adapters should translate ecosystem conventions into the generic ReproBisect model.

## 17.1 C/C++

Priorities:

- Make;
- CMake;
- Meson;
- Autotools;
- Ninja;
- compiler/linker provenance.

## 17.2 Python

Support:

- `pyproject.toml`;
- wheel/sdist;
- setuptools;
- hatchling;
- flit;
- Poetry/PDM where appropriate.

## 17.3 Rust

Support:

- Cargo workspaces;
- build scripts;
- feature matrices;
- `RUSTFLAGS`;
- remap-path-prefix;
- lockfile/toolchain provenance.

## 17.4 Go

Support:

- `go build`;
- module provenance;
- VCS stamping;
- `-trimpath`;
- CGO interactions.

## 17.5 JVM

Support:

- Maven;
- Gradle;
- JAR/WAR structural analysis;
- ZIP timestamps;
- manifest generation.

## 17.6 JavaScript/TypeScript

Support:

- npm/pnpm/yarn;
- lockfile variants;
- bundlers;
- generated chunks;
- archive packaging.

## 17.7 Bazel / Nix

Treat later because these systems already have strong hermeticity/reproducibility abstractions and may require a different integration model.

The goal should not be to replace them, but to diagnose reproducibility failures at their boundaries.

---

# 18. Phase 12 — Security hardening

**Timing:** Baseline controls before the 1.2 Action, exports, or model integration; advanced sandbox research remains later.  
**Release target:** baseline `1.2`; deeper isolation only after platform qualification.  
**Theme:** Running arbitrary builds is inherently dangerous.

Part III tasks P02–P04 implement the early boundary. Do not advertise OCI as a hostile-code sandbox. Resource limits and network policy must be held constant across baseline/intervention/reversion unless explicitly part of the experiment; reject conflicts with the CPU-count or network-access dimensions rather than silently overriding them.

## 18.1 Rootless execution by default where practical

Prefer:

- rootless Podman;
- Docker user mappings;
- dropped capabilities;
- `no-new-privileges`;
- read-only mounts where feasible.

## 18.2 Resource limits

Support configurable:

- CPU limits;
- memory limits;
- PID limits;
- disk limits where practical;
- build timeouts.

## 18.3 Network isolation

Network behavior should be explicit:

```text
network = "off"
network = "on"
network = "record"
```

New restricted profiles should lean toward reproducibility and safety, but do not silently change existing build-network semantics in a minor release. Separate provider egress from build egress; offline model operation does not imply that a configured build needs no network. Any new profile must name the policy in evidence and surface experiments it cannot perform.

## 18.4 Untrusted-repository mode

Eventually:

```bash
reprobisect check . --untrusted
```

could enforce stricter sandbox defaults.

This is especially important if users begin pointing ReproBisect at arbitrary GitHub repositories.

---

# 19. Phase 13 — Remote and distributed comparison

**Indicative window:** Months 10–15+  
**Release target:** `2.0` candidate  
**Theme:** Diagnose differences between environments that cannot be recreated locally.

This is a major architectural expansion and should not be rushed.

Potential workflow:

```text
Machine A / CI provider A
    ↓ evidence bundle

Machine B / CI provider B
    ↓ evidence bundle

reprobisect compare-env A.json B.json
    ↓
candidate environment deltas
    ↓
replay selected deltas in controlled runner
```

Use cases:

- reproducible locally but not in GitHub Actions;
- GitHub Actions vs GitLab CI;
- x86_64 vs ARM64;
- distro/toolchain drift;
- internal CI vs release builder.

This may eventually require remote runners or agents.

---

# 20. Phase 14 — Cross-platform expansion

**Indicative window:** after Linux maturity  
**Release target:** likely `2.x`

Linux containerized builds should remain the mature baseline first.

Possible expansion order:

1. Linux x86_64;
2. Linux arm64;
3. macOS-hosted orchestration;
4. Windows/WSL workflows;
5. native Windows experiments only if strong demand exists.

Cross-platform work must carefully distinguish:

- host platform;
- container platform;
- target platform;
- artifact format;
- build architecture.

Do not add “Windows support” as a superficial checkbox.

---

# 21. Quality metrics

The project should track its own maturity using measurable signals.

## 21.1 Correctness metrics

- false-positive diagnoses found in corpus;
- false-negative known-cause cases;
- baseline instability correctly detected;
- successful reversal confirmations.

## 21.2 Coverage metrics

Number of pinned external projects by:

- ecosystem;
- build system;
- artifact type;
- runner;
- architecture.

## 21.3 Performance metrics

For each corpus case:

- baseline build duration;
- total ReproBisect duration;
- number of builds executed;
- experiment reduction from adaptive scheduling;
- peak disk usage.

## 21.4 UX metrics

From issues and external testers:

- time to first successful run;
- percentage requiring manual config edits;
- common doctor failures;
- common unsupported dimensions.

No invasive telemetry is required; issue templates and corpus runs can provide much of this.

---

# 22. Documentation roadmap

Documentation should grow with the project rather than remain concentrated in the README.

Recommended structure:

```text
docs/
  getting-started.md
  concepts/
    causal-model.md
    experiment-model.md
    confidence.md
  guides/
    cmake.md
    python.md
    cargo.md
    github-actions.md
  artifacts/
    elf.md
    zip-wheel.md
    tar-ar.md
  advanced/
    interactions.md
    tracing.md
    performance.md
    llm-assistance.md
  reference/
    config.md
    evidence-schema.md
    exit-codes.md
  security/
    threat-model.md
    llm-privacy.md
```

The README should remain a front door, not become the entire manual.

---

# 23. Issue templates for the next development cycle

## 23.1 “ReproBisect failed to explain my build”

Ask for:

- repository / reproducer;
- build system;
- artifact path;
- expected cause if known;
- observed ReproBisect result;
- JSON report;
- runner;
- platform.

## 23.2 “False diagnosis”

Treat this as high priority.

Ask for:

- claimed causal variable;
- independent reason it is believed incorrect;
- evidence report;
- minimal reproducer.

## 23.3 “New experiment dimension”

Require:

- variable to perturb;
- why it can affect reproducibility;
- how to intervene;
- how to revert;
- potential confounders;
- example real projects.

This protects the causal model from becoming an arbitrary collection of toggles.

---

# 24. What not to prioritize yet

Several attractive ideas should remain deliberately deferred.

## 24.1 GUI

A GUI may eventually help, but the project first needs stable workflows and evidence semantics.

A GUI built now would mostly wrap moving targets.

## 24.2 Autonomous LLM root-cause claims

Do not let a model directly label causes without controlled experiments.

That would undermine the project's strongest technical property.

## 24.3 Huge artifact-format expansion for its own sake

Add formats when real external projects require them.

## 24.4 Native Windows parity

Not until Linux/container workflows are mature and there is demonstrated demand.

## 24.5 Hosted SaaS

A hosted service introduces:

- untrusted code execution;
- source confidentiality;
- storage;
- billing;
- sandboxing;
- supply-chain concerns.

That is a separate product problem.

## 24.6 A generic “AI debugger for any codebase”

Do not broaden ReproBisect into a generic code-reading/patch-writing debugger merely because the market for AI coding tools is large.

That direction would erase the project's clearest differentiator and place it against mature categories with different strengths.

If ReproBisect expands beyond builds, the expansion should remain **interventional and evidence-backed**:

```text
candidate cause
→ controlled change
→ measured oracle
→ repetition
→ reversal
→ minimal causal evidence
```

The regression research track in Section 8A is the approved way to test this broader opportunity.

---

# 25. Suggested implementation order — revised 2026-10-01

Use Part III's stable task IDs and dependency table as the execution queue. Earlier phase numbers group ideas, not dispatch order.

1. **P00:** establish the verified 1.1 implementation base and re-run its baseline gates. Publication is separate.
2. **P01 → P02 → P03:** automation envelope, bounded/cancellable execution, and safe disclosure.
3. **P04:** pinned least-privilege CI Action and qualified installation.
4. **P05 → P06:** expand measured external cases; establish a deterministic scheduler and fair baseline.
5. **A01 (optional):** read-only agent/MCP facade, reusing the same schemas and disclosure boundary. Not required to ship 1.2.
6. **M01 → M02 → M03 (optional research):** typed-decision protocol, local Laya shadow benchmark, and only then a promotion decision; Jev/Kev/AnyJev are comparators, not dependencies of the core.
7. **R01 (separate research):** narrow regression-oracle prototype after the stable CI/cost substrate. No model dependency.
8. Later: demand-led parser/ecosystem improvements, full reproducer minimization, verified generative proposals, and only then justified remote/native-platform expansion.

Security, privacy, compatibility, and review are gates at every step. Local-model support and egress controls come **before** hosted integrations, not at the end of an AI stage. Do not block core releases on optional model, MCP, or regression experiments.

---

# 26. Milestones instead of a promised 12-month calendar

| Milestone | Exit criterion |
|---|---|
| Qualified base | Exact 1.1 commit materialized separately; fresh locked/compiler/runtime gates recorded or explicitly blocked. |
| CI-ready 1.2 candidate | P01–P04 pass end-to-end on the exact candidate; no silent uploads or incomplete-success statuses. |
| Practical investigation baseline | P05–P06 show usefulness and measured cost on external cases, including negative controls. |
| Optional decision-model pilot | M01–M02 show safe fallback and independently measured local task quality; no automatic promotion. |
| Optional learned scheduling | M03 meets the predeclared practical-value gate without causal/coverage regression. |
| Optional regression product | R01 feasibility first, then wider unrelated corpus and a deliberate scope decision. |

Release numbers remain planning labels. No calendar date, vendor benchmark, or number of implemented features substitutes for these gates.

---

# 27. Definition of “production-ready”

ReproBisect should not call itself production-ready merely because it has many features.

A reasonable bar would include:

- substantial external corpus across ecosystems;
- no known high-severity false-positive class;
- stable configuration/evidence compatibility policy;
- predictable CI integration;
- documented security model;
- bounded resource behavior;
- strong error diagnostics;
- reproducible release process;
- proven operation on large projects;
- performance acceptable for routine CI or targeted debugging;
- multiple external users successfully diagnosing issues that were not designed into the test suite.

The strongest signal will be:

> **External maintainers use ReproBisect to discover causes the ReproBisect authors did not already know.**

That is the point where the project has demonstrated genuine generalization.

If software-regression debugging becomes a supported product mode, add a separate readiness bar before calling that mode production-ready:

- real regression corpus across ecosystems;
- explicit flaky-oracle handling;
- no known high-severity causal misattribution class;
- safe patch/revert/transplant semantics;
- transparent handling of inapplicable interventions;
- interaction-aware reporting;
- evidence compatibility for code-change interventions.

---

# 28. Final strategic recommendation

**Current update (2026-10-01):** upstream 1.1 remains qualified and unpublished; first establish the correct development base rather than work on the older local RC. The strongest engineering move is the safety-first 1.2 CI slice in Part III, alongside external validation. A local typed-decision shadow trial follows the deterministic scheduler/benchmark—not a generative-agent rewrite. Publication remains a separate owner decision.


The project should resist two opposite failure modes.

The first is **underbuilding**:

> publishing `v1.0.0`, receiving little immediate feedback, and leaving the tool essentially frozen.

The second is **overbuilding**:

> immediately adding a GUI, an LLM agent, remote workers, dozens of formats, and multiple platforms before understanding what early users actually need.

The better loop is:

```text
publish
   ↓
observe real failures
   ↓
turn failures into fixtures
   ↓
improve UX or causal coverage
   ↓
verify against old + new corpus
   ↓
release again
```

LLMs fit naturally into this strategy, but only after the deterministic core is easy to operate.

The near-term product vision, refined by the October review, remains:

> **ReproBisect should become a first-class causal investigation engine for builds: deterministic at the evidence layer, adaptive at the search layer, artifact-aware at the analysis layer, CI-native in operation, optionally decision-model-assisted at the ordering layer, and optionally generative-model-assisted at the proposal/explanation layers.**

A larger long-term hypothesis is now worth testing:

> **ReproBisect may be able to become an experimental causal debugger for software regressions, where builds are the first domain rather than the only domain.**

The project should earn that broader description rather than announce it in advance.

The proof should come from a regression prototype that can repeatedly show results such as:

```text
known-good behavior
known-bad behavior
candidate software change
remove from bad → failure disappears
introduce into good → failure appears
repeat / reverse / minimize
→ auditable causal evidence
```

If that works across a substantial unrelated corpus—including interaction cases—then a more general architecture around `Subject / Intervention / Runner / Oracle / Causal Engine / Evidence` is justified.

If it does not, ReproBisect should remain a focused build-causality tool. That is still a coherent and useful product.

The strategic rule is therefore:

> **Broaden only along the causal-experiment axis, never merely along the “AI can inspect more code” axis.**

That preserves the project's technical identity while giving it a credible path to a much larger problem space.

---

# Part III — Implementation handoff for the reviewed plan

> **For Hermes:** Execute task-by-task with fresh spec-compliance and code-quality review of the exact diff. Load the subagent-driven-development workflow if available; otherwise use the established review fallback. Failed, incomplete, provider-failed, or stale reviews are not approvals. This document is a plan, not permission to begin implementation or publish a release.

**Goal:** Make ReproBisect safely useful from CI and agent clients, then determine whether an optional typed-decision ranker reduces the cost of confirmed causal investigation.

**Architecture:** Preserve the single Rust crate and deterministic OCI experiment engine. Add small orchestration, disclosure, and scheduling seams around it; isolate optional decision runtimes behind a strict advisory protocol. Existing report schemas remain authoritative and compatible; new planning/advisory/export records are explicitly versioned separately.

**Tech stack:** existing Rust 2024/MSRV 1.85.1 qualification, Docker/Podman, Python stdlib tooling for corpus/evaluation; optional separately pinned Python model runtime. No mandatory AI dependency, no database/service architecture without demonstrated need.

## Execution rules and test commands

All file paths below are relative to the **verified upstream 1.1 development worktree**, not this outer folder or its old RC checkout. `Modify` paths exist in that pinned tree; `Create` paths are proposals. Tests named below are acceptance targets to write, **not tests claimed to exist or pass today**.

For each code behavior, take small vertical steps:

1. Write the named regression/public-boundary test first, including failure and negative control.
2. Run the named test and confirm an expected behavioral failure (not an import/environment failure).
3. Add the minimal implementation; rerun the test, then adjacent tests.
4. Refactor while green; run full relevant gates and obtain fresh hash-bound review.
5. Commit only that verified unit on the development branch; never amend the qualified release identity. Proceed only after approval of prerequisites.

Commands, after P00 establishes the correct worktree:

```bash
python scripts/static-check.py
python scripts/real-world-corpus.py validate
cargo +1.85.1 check --locked --all-targets
cargo +1.85.1 test --locked --all-targets
cargo +stable check --locked --all-targets
cargo +stable test --locked --all-targets
bash scripts/test-fixtures.sh
bash scripts/test-podman.sh
```

Use `cargo +1.85.1 test --locked <test_name>` for each named Rust test; verify the named test actually ran (a zero-test filter is not a pass). Runner/engine changes require both OCI fixture suites and relevant pinned real-world cases; release candidates require the full corpus and release workflow on the exact tree. Run container gates on a qualified Linux environment, not by assuming Windows-host parity. Record blocked gates; do not substitute static checks. These commands were **not** executed in this review.

## Dependency schedule

| Task | Depends on | Track |
|---|---|---|
| P00 | none | Base prerequisite |
| P01 | P00 | Core automation |
| P02 | P01 | Core execution controls |
| P03 | P01, P02 | Core disclosure |
| P04 | P02, P03 | Core CI/distribution |
| P05 | P04 | External-use/cost baseline |
| P06 | P02, P05 | Deterministic scheduling |
| A01 | P01, P03, P04 | Optional read-only agent adapter |
| M01 | P03, P06 | Optional model contract |
| M02 | M01 | Optional local shadow trial |
| M03 | M02 | Optional comparator/promotion gate |
| R01 | P02, P04, P05, P06 | Separate regression research |

A01, the M-track, and R01 do not block the independently useful core. Hosted API access is **not** a dependency of local promotion; an unavailable comparator is reported as not run. R01 does not wait for model work.

### Task P00 — Establish the correct development base

**Objective:** Prevent new work being built on the old local RC or silently changing a qualified release.

**Files:**
- Inspect: `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `src/schema.rs`, `docs/implementation-status.md`.
- Modify: `docs/implementation-status.md` only on the future development branch, if needed to distinguish fresh results from historical qualification.

**Validation:** Recheck remote refs, package/lock versions, clean/staged state, and expected tree. Deliberately create a separate worktree from `ee015fa22a261baddb3af2dad75e81783a55f339`; do not reset the existing checkout. Run the baseline commands above. Preserve existing staged/dirty work and record source/tree hashes.

**Gate:** exact implementation base and fresh available gate results documented; unavailable compiler/runtime gates are explicit blockers, not inferred from historical GitHub success. Publication/tagging still needs separate authorization.

### Task P01 — Freeze an automation envelope without changing diagnostic truth

**Files:**
- Create: `src/ci.rs`.
- Modify: `src/cli.rs`, `src/main.rs`, `src/report.rs`, `tests/cli.rs`, `docs/evidence-compatibility.md`.

**First RED tests:** `ci_json_stdout_is_one_document`; `ci_policy_preserves_diagnostic_status`; `ci_missing_report_is_not_success`; `ci_incomplete_coverage_is_visible`.

**Implementation:** Define a versioned orchestration envelope around the existing report. Separate diagnostic status, operation completion, coverage, and caller policy result; make JSON stdout clean and progress stderr-only. Preserve default `0/1/2/5` behavior and old evidence readers. Do not invent a finding when only an infrastructure error exists.

**Gate:** CLI subprocess tests parse actual stdout for success, finding, invalid usage, operational failure, and incomplete run; assertions cover status *and* exit code. Historical schema matrix and full Rust tests remain green. Document whether cancellation uses the existing operational-error exit with an explicit envelope reason before freezing any new CLI behavior.

### Task P02 — Bound, cancel, and account for execution before dispatch

**Files:**
- Create: `src/engine/budget.rs`.
- Modify: `src/config.rs`, `src/engine/mod.rs`, `src/engine/control.rs`, `src/engine/compare.rs`, `src/engine/fix.rs`, `src/runner/oci.rs`, `src/output_discovery.rs`, `src/doctor.rs`, `src/ci.rs`, `tests/cli.rs`, `SECURITY.md`.

**First RED tests:** `budget_exhaustion_never_reports_reproducible`; `cancel_kills_owned_container_and_keeps_partial_evidence`; `resource_policy_conflict_is_rejected`; `image_probe_deadline_is_bounded`.

**Implementation:** Add one shared monotonic deadline/run-count budget covering baseline, interventions, subset search, confirmations, fix verification, discovery, and infrastructure probes. Reserve a complete confirmation group before promoting a candidate; consume launched work even when cancelled or its result is lost. Bound image inspect/pull/probes as well as build commands. Pass a cancellation handle and uniquely owned container identity through runner entry points; never kill unrelated runtime jobs.

Hold memory/PID/CPU/network policy constant across comparison arms, and validate conflicts with the tested dimension. Keep tracing privileges explicit. Bounds on parsed trace bytes do not bound temporary trace disk: enforce a defensible storage cap or disable tracing in the restricted profile. Emit partial evidence plus completion reason, never successful full coverage. Do not introduce automatic resume/retry of unknown-outcome builds in this slice; a later recovery feature needs explicit receipts and state reconciliation.

**Gate:** actual Docker and Podman timeout/cancellation fixture leaves no owned running container, preserves parseable partial records, and leaves the checkout untouched. Tests cover zero/invalid budgets, deadline exhaustion during baseline and reversal, infrastructure hangs, and cumulative allowance. Full same-policy fixture/corpus results retain causal semantics. Strong hostile-code containment is still not claimed.

### Task P03 — Safe derived summaries and explicit disclosure

**Files:**
- Create: `src/disclosure.rs`.
- Modify: `src/main.rs`, `src/ci.rs`, `src/report.rs`, `src/evidence.rs`, `tests/cli.rs`, `SECURITY.md`.

**First RED tests:** `summary_omits_raw_logs_and_controlled_secrets`; `summary_escapes_workflow_commands`; `export_preserves_original_evidence_digest`; `export_refuses_symlink_and_parent_escape`.

**Implementation:** Define an allowlisted summary view shared by CI, support bundles, and later models. Exclude raw logs, source/binaries, absolute/private paths, ordinary environment values, and all free-form controlled values by default; disclose exact values only through an explicit field allowlist/preview, never a claim that a secret detector proved them safe. Preserve private canonical evidence locally. Preview selected disclosure and bind consent to the exact payload hash/destination; changed payloads invalidate consent. For unattended CI, retain newly generated evidence only on the runner pending exact-payload approval; never interpret a static upload flag as that approval. Automatic upload requires a separately designed standing-authorization contract and is deferred. Mark excluded/redacted fields and distinguish original versus derived hashes. Share bundles are bounded, create-only, and refuse traversal/symlink escapes.

**Gate:** adversarial secret/control-string fixtures pass against actual rendered summaries and bundle manifests; original evidence remains byte-identical. Default use makes no uploads/network calls. Document residual disclosure risk of filenames, digests, package metadata, and explicit experimental values rather than promise total anonymization.

### Task P04 — Pinned least-privilege GitHub Action and installation

**Files:**
- Create: `action.yml`, `scripts/run-action.py`, `scripts/test-action.py`, `.github/workflows/action-integration.yml`.
- Modify: `.github/workflows/release.yml`, `scripts/release-check.py`, `README.md`, `docs/release-qualification.md`.

**First RED tests:** `test_checksum_mismatch_fails_before_execution`; `test_missing_report_fails_closed`; `test_upload_is_off_by_default`; `test_upload_flag_without_exact_payload_approval_is_denied`; `test_untrusted_input_is_not_shell_source` in `scripts/test-action.py`.

**Validation command:** `python scripts/test-action.py` (new stdlib test harness), followed by a real Linux workflow against the reviewed candidate binary. Include a finding case and a deliberately interrupted case, not only a green fixture.

**Implementation:** Wrap existing CLI only. Pin binary version/checksum and action dependencies; verify runtime/architecture before launch. Propagate explicit budget/policy settings; write escaped outputs using GitHub file commands. Read-only token by default, no secret access in untrusted builds, no privileged untrusted checkout, and no implicit evidence upload. Reuse P03 summaries rather than another sanitizer.

**Gate:** an exact-commit workflow proves installation, clean JSON/status policy, bounded failure handling, summary rendering, and runner-local artifact retention pending exact-payload disclosure approval (ephemeral runners may discard it at teardown). No unattended remote upload in this slice. Release records identify source, package, binary digest and qualification. Arm64 remains separately unqualified until its native matrix passes; public release is not part of this task.

### Task P05 — Measure external usefulness and build a decision benchmark

**Files:**
- Create: `corpus/decision/manifest.json`, `scripts/evaluate-decisions.py`.
- Modify: `scripts/real-world-corpus.py`, `corpus/README.md`, `docs/real-world-validation.md`.

**First RED tests:** `test_repository_splits_do_not_overlap`; `test_future_outcomes_are_excluded_from_features`; `test_unobserved_candidate_is_not_negative`; `test_infra_failure_is_not_causal_label` in the new evaluator's stdlib self-test suite.

**Validation command:** `python scripts/evaluate-decisions.py --self-test` (proposed new command).

**Implementation:** Retain the pinned seven-case release corpus; add a small demand-led pilot of unrelated maintained projects/build workflows before aiming for the broader historical 15–30-project target. Record setup effort, human review time, first useful diagnosis, total builds, budgets, exact input identities and unsupported cases. Include wheel/JAR compressed metadata, compiler paths, dependency/image drift, interactions, negative controls, and unstable/infra-failed runs; select cases from observed need, not just easy synthetic successes.

For model evaluation, derive pre-decision feature views and candidate outcomes from real executed evidence. No model-written “root cause” labels. Split by repository/project lineage (including forks/variants) into train, calibration, and frozen holdout; keep synthetic safety tests separate. Minimized or early-stopped histories have missing counterfactuals—retain them as unknown rather than manufacture labels.

**Gate:** no evaluation leakage, exact provenance, consent for any retained proprietary cases, and a reproducible no-model cost/quality baseline. Small pilot findings support feasibility only, not general production claims.

### Task P06 — Deterministic cost-aware scheduling and honest coverage

**Files:**
- Create: `src/engine/scheduler.rs`.
- Modify: `src/engine/mod.rs`, `src/engine/interventions.rs`, `src/engine/control.rs`, `src/ci.rs`, `src/report.rs`.

**First RED tests:** `rules_schedule_preserves_candidate_set`; `first_finding_retains_unrun_coverage`; `baseline_and_reversal_cannot_be_ranked_away`; `scheduler_does_not_reuse_build_observations`.

**Implementation:** Separate eligibility from ordering; use existing `PlannedIntervention` objects, deterministic artifact/config features, measured cost estimates, and stable tie-breaking. Persist planned/executed ordering and reasons in the envelope. Keep the existing fixed order as a selectable benchmark baseline. Begin serially; parallelism/caching need separate interference qualification. Only use completed evidence, not future ground truth, when ordering.

**Gate:** fixed and rules schedules both retain negative controls and required confirmation; they agree on fully explored deterministic cases. Any first-finding or budget-limited run declares incomplete coverage. Compare build count and wall time on P05 cases; retain fixed order if added scheduling complexity is not useful.

### Task A01 — Optional read-only agent/MCP facade

**Files:**
- Create: `integrations/mcp/README.md`, `integrations/mcp/server.py`, `integrations/mcp/test_server.py`.

**First RED tests:** `test_unknown_tool_is_rejected`; `test_report_path_escape_is_rejected`; `test_read_only_tools_cannot_spawn_build`; `test_structured_result_matches_schema`.

**Validation command:** `python -m unittest discover -s integrations/mcp -p 'test_*.py'` (new adapter tests).

**Implementation:** Only if actual clients benefit beyond CLI JSON, wrap existing persisted report reading and sanitized summaries in a minimal protocol adapter. Fixed workspace root, bounded reads, no shell tool, arbitrary URL fetch, model provider, build launch, or checkout editing. Return validated structured data and evidence links. MCP specifies input/output schemas and validation/security expectations; protocol annotations alone are not authorization.[15]

**Gate:** one real client round-trip and denied-operation tests; no duplicate causal engine or renderer. Future execution tools need a separate approval-bound plan/run API using P02, not a boolean supplied by a model. Defer this adapter if ordinary subprocess invocation is sufficient.

### Task M01 — Advisory decision protocol, feature view, and shadow hook

**Files:**
- Create: `src/decision/mod.rs`, `src/decision/protocol.rs`.
- Modify: `src/main.rs`, `src/disclosure.rs`, `src/engine/scheduler.rs`.

**First RED tests:** `off_mode_never_invokes_provider`; `unknown_candidate_and_nonfinite_scores_are_rejected`; `shadow_prediction_cannot_change_schedule`; `advice_cannot_set_diagnosis_confidence`; `changed_candidate_digest_invalidates_advice`.

**Implementation:** Implement Section 15's minimal typed-ranker request/response, a deterministic fake for tests, and versioned advisory sidecars. Distinguish “abstain” from “no cause”; tie predictions to the exact evidence/candidate set. Do not route canonical evidence through an AI schema. Inference errors remain separate from experiment outcomes. Define bounded input preprocessing with provenance and token-overflow refusal.

**Gate:** deterministic reports/outcomes/ordering are unchanged in shadow mode; no hidden network/runtime dependency in default mode; malformed/adversarial advice cannot launch any action or modify a verdict. Process-protocol caps and timeout behavior are tested before real model loading.

### Task M02 — Explicit local Laya adapter and domain evaluation

**Files:**
- Create: `integrations/decision/laya_adapter.py`, `integrations/decision/requirements.lock`, `integrations/decision/test_laya_adapter.py`, `integrations/decision/README.md`.
- Modify: `src/decision/mod.rs`, `src/decision/protocol.rs`, `scripts/evaluate-decisions.py`, `corpus/decision/manifest.json`.

**First RED tests:** `test_missing_checkpoint_does_not_download`; `test_offline_mode_denies_egress`; `test_option_truncation_abstains`; `test_act_probability_is_not_authority`; `test_timeout_falls_back_to_rules`.

**Validation command:** `python -m unittest discover -s integrations/decision -p 'test_laya_adapter.py'`, then an explicitly provisioned real-checkpoint smoke and P05 development/calibration evaluation. Keep the final repository-disjoint holdout sealed until M03. A mock-only pass is insufficient.

**Implementation:** Pin reviewed runtime/checkpoint/tokenizer/calibration digests; install/download only by a separate explicit setup action. Load a local snapshot with outbound networking blocked. First support a bounded subprocess using pre-provisioned interpreter/runtime; avoid a persistent HTTP server unless needed. Route no fallback to cloud. Keep model loading/inference outside timed OCI experiments. Benchmark base and relevant specialized checkpoints rather than assuming the typed-decisions name means build-domain competence. Test neutral option labels, reordered options, short/long/truncated inputs, repeated calls, mixed-language/code/log content, and confident wrong answers.

**Gate:** CPU-capable real inference with recorded cold/warm latency, peak RAM, disk/download size and hardware; no unexpected egress; stable typed output and deterministic fallback. Report development/calibration accuracy, ranking and abstention, explicitly labeled non-confirmatory. Select checkpoints, input templates, option ordering policy, abstention thresholds and calibration using development/calibration groups only. Fine-tuning on permitted labels is optional and later; never fit or select against the final holdout. No production scheduler influence at this gate.

### Task M03 — Compare alternatives and decide whether to promote

**Files:**
- Create: `integrations/decision/jev_adapter.py`, `integrations/decision/test_jev_adapter.py`, `docs/decision-evaluation.md`.
- Modify: `scripts/evaluate-decisions.py`, `src/decision/mod.rs`, `src/engine/scheduler.rs`, `src/ci.rs`.

**First RED tests:** `test_hosted_without_payload_consent_is_denied`; `test_remote_retry_respects_total_budget`; `model_order_cannot_drop_required_trials`; `model_failure_restores_rule_order`.

**Implementation:** Jev is an **optional comparator** only after explicit payload/provider/spend approval. Pin requested and returned model identity; reuse M01's schema and P03's bounded view; handle 401/429/timeouts with bounded attempts. Test that provider confidence fields are not assumed interchangeable. If no credentials or approval, mark hosted evaluation not run and continue local go/no-go. Kev/AnyJev or a small supervised baseline can be added only if they answer a concrete unresolved question; their runtime costs and licenses are evaluated separately.

**Benchmark modes:** fixed schedule, rule schedule, local decision model, optionally hosted decision model, and optionally a generative ranker on equal candidate/evidence/budget conditions. Report cold start, inference latency, complete investigation wall time, builds until first confirmed finding, completed-coverage fraction, causal precision/recall on adjudicated cases, ranking recall, calibration/Brier or ECE, abstention/coverage, RAM/VRAM, hosted tokens/cost, egress bytes, and human setup/review burden. Before any final holdout is opened, freeze all compared checkpoints/templates/calibration/abstention policies, the common eligible cohort, budgets, baseline-selection rule, metrics and promotion threshold. A protocol change after opening results requires a fresh repository-disjoint confirmation set. Group resampling by repository, not by near-duplicate question. Keep replay estimates separate from actual paired executions. Include unsolved, timeout, and budget-exhausted cases in the denominator; report censored time/build counts and solve rate at the fixed cap instead of dropping difficult cases from medians. Compare both equal-budget and completed-coverage results so a faster but incomplete run cannot win by doing less.

**Predeclared promotion gate:** zero model-caused authority/coverage violations in the adversarial and regression suites; no decrease in confirmed-finding success at the common budget on the complete predeclared positive cohort, including unfinished cases; and a target **at least 15% reduction in capped builds-to-confirmation versus the better fixed/rule baseline selected on development data**, measured as one minus the ratio of cohort medians (model / baseline). Assign every unsuccessful positive investigation the common build cap for this cost metric, retain its failure indicator, and also report censored time-to-confirmation; never exclude it. Report negative-control correctness and completion cost separately. Require this improvement with no increase in median end-to-end wall time after loading/inference overhead and no lower confirmed-finding rate at the common budget cap. Report uncertainty intervals and the full case distribution; if the holdout is too small for a credible conclusion, remain experimental regardless of the point estimate. This 15% is a proposed practical-value threshold, not a measured result; freeze the complete protocol before opening holdout results, not merely this threshold.

**Gate:** publish the evidence table internally and make a go/no-go decision. A pass enables explicitly opted-in **reorder-only** operation; a fail leaves models in shadow/off mode and ships the deterministic core. No pruning, shorter controls, auto-approval, or “model verified root cause.” Hosted superiority never silently changes the default away from local/off.

### Task R01 — Regression-oracle feasibility, independent of AI

**Files:**
- Create: `src/regression/mod.rs`, `src/regression/oracle.rs`, `src/regression/patches.rs`, `corpus/regressions/README.md`.
- Modify: `src/main.rs`, `src/cli.rs`, `src/engine/ddmin.rs`, `tests/cli.rs`.

**First RED tests:** `flaky_oracle_refuses_minimization`; `invalid_transplant_is_unresolved_not_failure`; `oracle_file_is_not_changed_by_candidate`; `regression_trial_leaves_checkout_unchanged`.

**Implementation:** Narrow explicit good/bad revision plus executable-oracle prototype in isolated snapshots. Pin oracle/harness identity outside the change set; a patch must not “fix” a case by deleting or weakening its test. Distinguish PASS, target FAIL, UNRESOLVED/invalid application, timeout, and infrastructure failure. Bound execution through P02. Reuse existing ddmin only through an outcome-aware seam; do not force invalid transplants into its failure predicate. Establish controls, remove from bad, introduce onto good where valid, repeat/reverse, and record inapplicability.

**Gate:** feasibility on 3–5 unrelated real regressions before expanding; compare against `git bisect run`, ordinary subset delta debugging, and manual investigation cost. A stable test failure is a behavioral oracle, not artifact equality. Only proceed to the earlier 20–30-case product-promotion target if localization is materially more informative at acceptable cost and all build-mode regression gates remain green. No generic-debugger repositioning or generalized-framework rewrite at this stage.

## Deferred items and explicit non-goals

- No AI root-cause judge, security authorizer, log redactor of last resort, or replacement for deterministic arithmetic/statistics.
- No automated patch application to the user's checkout; generative proposals need a separately detailed safety/verification task before implementation.
- No claim that an MCP facade, OCI container, typed JSON, signature, or model calibration is proof of causal correctness or hostile-code containment.
- No telemetry by default, model downloads on ordinary CLI use, paid-provider dependency, or cloud fallback.
- No new evidence database, distributed runner, universal plugin framework, native Windows parity, SaaS, or GUI before a real need justifies it.
- No experiment result reuse to simulate repetition. No plan-only claim of tests, model accuracy, speedups, or release readiness beyond the specifically rechecked records.

## Plan verification and handoff checklist

- Preserve the old release/tag/hash record and clearly distinguish local, upstream, historical, proposed, and freshly tested state.
- Validate unique task IDs, exact schedule coverage, acyclic prerequisites, named tests/gates, and file-target existence or earlier creation.
- Verify every new external citation against retrieved sources; vendor measurements remain attributed, not promoted to ReproBisect results.
- Confirm documentation-only edits: nested tracked files, Git refs/index, and unrelated publication drafts unchanged.
- Before implementation, recheck sources/versions and obtain an exact-worktree baseline; before each dependent task, require fresh narrow approval bound to its actual changed hashes.

## Sources

[1] https://api.github.com/repos/trisanu-das/ReproBisect/releases/latest
[2] https://api.github.com/repos/trisanu-das/ReproBisect/actions/runs/35350058321
[3] https://api.github.com/repos/trisanu-das/ReproBisect/actions/runs/35350058382
[4] https://raw.githubusercontent.com/trisanu-das/ReproBisect/ee015fa22a261baddb3af2dad75e81783a55f339/Cargo.toml
[5] https://docs.typesafe.ai/introduction/coding-agents.md
[6] https://docs.typesafe.ai/confidence.md
[7] https://docs.typesafe.ai/models.md
[8] https://docs.typesafe.ai/api.md
[9] https://docs.typesafe.ai/model-jaggedness/jev-1.13.md
[10] https://huggingface.co/convaiinnovations/laya/raw/main/README.md
[11] https://raw.githubusercontent.com/NandhaKishorM/laya/main/README.md
[12] https://raw.githubusercontent.com/jaredpalmer/kev/main/README.md
[13] https://raw.githubusercontent.com/nokia-applied-research/AnyJev/main/README.md
[14] https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions
[15] https://modelcontextprotocol.io/specification/2025-11-25/server/tools



