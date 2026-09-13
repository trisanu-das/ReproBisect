# Experiment Model

Every compared run is modeled as:

```text
(source snapshot, build spec, controlled environment) -> artifacts
```

## Claim discipline

A one-variable causal diagnosis requires, at minimum:

1. repeated baseline builds are byte-stable,
2. the intervention is explicit and recorded,
3. for a byte-level non-reproducibility claim, the intervention changes one or more declared artifacts,
4. a repeatable network-disabled build failure may instead support a scoped network-dependence diagnosis while leaving byte reproducibility inconclusive,
5. the conclusion is scoped to the tested build and intervention.

Confidence increases when:

- repeated variant runs agree,
- a baseline reversion restores the original artifact hashes,
- the changed artifact directly contains a controlled value associated with the intervention.

The tool does not infer causality from an embedded string alone. A container-image intervention is additionally treated as a bundled/coarse cause: it can establish sensitivity to the image but not isolate a compiler, linker, libc, or package. Same-image executable bindings (`toolchain:CC`, `toolchain:AR`, etc.) are narrower still. Phase 8 requires the stable wrapper for a controlled tool to record at least one actual invocation on both sides before executable selection can receive high confidence; merely exporting a different binding is insufficient. One-file dependency declaration replacements remain claims about the controlled file bytes, not every transitive package.

## Baseline instability

If repeated runs under the same canonical baseline differ, the current engine reports `UNCONTROLLED_NONDETERMINISM` and stops the deterministic one-variable sweep. That establishes instability under the tested controls; it does not prove that the build is inherently random under all environments.

For an opt-in CPU-count intervention, ReproBisect performs an additional fixed-size stochastic probe after the initial gate. It interleaves baseline-reference and CPU-variant builds and classifies each against the original stable artifact signature. If a later baseline reference changes, the initial gate is considered contradicted and the report is downgraded to `UNCONTROLLED_NONDETERMINISM`; deterministic diagnoses and interaction minimization are suppressed.

When the matched baseline remains stable, a one-sided Fisher exact test asks whether changed-artifact trials are enriched in the CPU variant group. `p <= stochastic_alpha` with a larger variant rate is recorded as supported stochastic evidence. This is a fixed-sample categorical test, not a model of scheduler dynamics, and it does not identify the internal race or task-order defect.

## Reproducibility scope

A successful result is reported as reproducible **within the tested space**. Image variants, narrow toolchain bindings, dependency-file variants, and network-off execution are opt-in. Dependency hashes, post-build resolution summaries, pre/post cache-content fingerprints, network syscall classes, and toolchain probes are provenance—not hermeticity proofs. In particular, a changed package-manager cache does not prove a network download, and a network syscall does not identify which content influenced the artifact. Untested factors can still matter, including subordinate tools, package-manager resolution behavior, remote content, kernel/filesystem behavior, CPU architecture/features, and other host state.

## Bounded cache and network evidence

Dependency-cache summaries are observational provenance, not dependency-resolution truth. Each cache snapshot has a file-count and cumulative-byte hashing budget. If either side reaches its budget, `comparison_complete=false`; the run may record `observed_change=true`, but `changed_during_build` remains false because the complete cache state was not observed.

If network tracing and runtime dependency provenance are both enabled, the run may also record build-window co-occurrence between successful non-local network syscalls and complete cache mutations. This means only that both phenomena were observed during the same build window. It does not establish temporal ordering at object granularity, identify a remote endpoint, or prove that a particular cached digest originated from the network.

## Process-aware provenance

`file_input_trace` is an opt-in observational channel. Successful opens are classified as dependency declarations, known/configured dependency-cache inputs, declared-output writes, or ephemeral non-output writes. Descriptor classifications remain internal to trace reduction: they can be inherited by a traced child, followed through `dup*`, and associated with a later readable `mmap`. Successful rename-family syscalls into a declared output are recorded as output publication; when the ephemeral source path has a known writer, the summary distinguishes same-process and same-lineage temporary publication. Failed file operations do not contribute positive evidence.

Only anonymous process keys, anonymous parent links, lineage depth, coarse roles, and counters are persisted. Besides same-process co-occurrence, the reducer records whether a strict traced ancestor observed a dependency, cache, or successful non-local network event before a descendant process exhibited output activity. Raw PIDs, paths, descriptor numbers, argv, and syscall text are discarded.

When `network_trace` is enabled simultaneously, only successful endpoints whose scope is known to be non-local (`public`, `private`, or `link_local`) can contribute to stronger process/lineage network correlations. `unknown`, loopback, and Unix-domain activity remain observational network counts but are not promoted to non-local correlation evidence.

The claim is intentionally narrow: **process/lineage co-occurrence is not taint analysis**. The model does not prove that read or mapped bytes flowed into written/published bytes. IPC/shared memory, `close_range`, platform-specific mapping syscalls, arbitrary in-memory transformations, and unresolved relative paths under changing cwd/dirfd state can remain invisible or ambiguous. A truncated syscall trace also prevents any completeness claim.


## Type-aware artifact structure

Artifact equality remains SHA-256 equality. Type-aware analysis is supporting evidence only. Phase 12 recognizes PE/COFF, Mach-O, and WebAssembly headers and refines structurally valid archive containers into JAR, Python wheel, DEB, or OCI-image types. The same bounded archive-member metadata comparison is used for generic and ecosystem-specific containers.

A type label is not a causal claim. For example, a changed wheel member timestamp strengthens a source-time intervention only because the intervention changed the artifact and the archive delta localizes the changed metadata; merely identifying a file as a wheel establishes nothing about why its bytes differ. Package payload contents are not executed or semantically trusted by the analyzer.

Phase 17 adds bounded semantic summaries and field-level semantic deltas. The semantic parser never executes artifact code and refuses oversized inputs. Selected small metadata records may be parsed only when their encoding can be handled under an explicit bound. Most semantic deltas are explanatory evidence only. The first deliberately narrow direct-evidence rule is `SOURCE_DATE_EPOCH -> PE/COFF coff_timestamp`: if the controlled epoch changes and the PE COFF timestamp field changes with it, that structural localization may strengthen the timestamp diagnosis. This remains evidence about the tested intervention, not proof that all timestamp-bearing bytes have been found.

## Runner identity

Every build attempt records the configured OCI runner backend (`docker` or `podman`). All control and intervention runs in one check use the same backend, including fix verification. Runner backend is not currently eligible for ddmin or one-variable causal attribution because changing container runtime is a broad compound intervention.

## Known-good / known-bad comparison predicate

The known-good endpoint must repeatedly succeed with one stable declared-artifact signature `G`. The known-bad endpoint may have either a stable artifact signature `B`, with `G != B`, or a stable build-failure signature `F`. Let `D` be the supported controlled-environment difference set. For a candidate subset `S ⊆ D`, ReproBisect constructs `E_S` by starting from the known-good environment and applying only the known-bad values in `S`.

For artifact-producing bad endpoints, the ddmin predicate is true only when repeated builds under `E_S` are internally stable **and** every declared-artifact hash equals `B`. `artifact(E_S) != G` is not sufficient because it could be a third outcome.

For build-failure bad endpoints, define:

```text
F = (exit_code, sha256(stdout), sha256(stderr))
```

The ddmin predicate is true only when every repeated attempt exits non-zero and reproduces exactly `F`. Matching the exit code alone is insufficient. Mixed success/failure candidates, or repeated failures whose diagnostic hashes differ, do not satisfy the predicate. Infrastructure/runner failures remain errors and are never converted into build-failure evidence.

Endpoint and subset repetition counts are configurable but never permitted below two. A post-minimization known-good reversion is optional (0 or 1); it must successfully recover `G`, and a failure or changed artifact signature makes the result inconclusive.
