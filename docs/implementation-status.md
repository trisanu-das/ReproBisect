# ReproBisect implementation status — 1.1 release candidate

**Status date:** 2026-09-18  
**Release target:** `1.1.0`  
**Branch:** `develop/1.1.0`  
**Scope:** adoption/diagnostic UX, project initialization, output discovery, terminal reporting, and release hardening without persisted-evidence schema changes

## Summary

The planned 1.1 feature slice is implemented. ReproBisect's causal experiment engine, evidence schemas, exit semantics, and stable 1.0 behavior remain intact; 1.1 focuses on making the existing engine easier to configure, validate, and understand.

The package is now marked `1.1.0` for final-tree qualification. This source-version promotion does not create a tag or public release.

## 1.1 features implemented

### `reprobisect doctor`

`doctor` performs preflight checks before expensive experiments and supports both text and JSON output. It distinguishes PASS/WARN/FAIL/SKIP states, checks the configured Docker/Podman runtime, and treats a missing local image as a warning rather than a false readiness failure.

### Build-system-aware `init`

`init` deterministically detects common project/build markers and proposes editable build configuration for Cargo, Go, npm/pnpm, Maven, Gradle, Bazel, Meson, CMake, Autotools, Python packaging, and Make.

Inference is explicitly labelled high/medium/low confidence and ambiguous multi-marker projects are surfaced rather than silently resolved as authoritative.

### Opt-in output discovery

`reprobisect init . --discover-outputs` performs one detected build in a copied temporary workspace, compares before/after filesystem snapshots, filters common intermediates, classifies candidate executables/packages/archives, and prints a ranked shortlist.

Only high-confidence candidates may replace statically inferred `build.outputs`; weaker candidates remain advisory.

The probe is bounded:

- at most 100,000 files per snapshot;
- at most 12 ranked candidates;
- 16 KiB retained tail per stdout/stderr stream;
- 600-second build watchdog;
- named temporary container killed on timeout.

The original checkout is not used as the build workspace.

### Diagnosis-first terminal report

Default text output now leads with result, promoted cause, confidence, affected artifacts, baseline/variant/reversion hashes where available, structural evidence, remediation, and tested-variable effect/no-effect summaries. `-v` retains the deeper experiment, provenance, trace, and build-log view.

## Compatibility

No persisted evidence schema versions are changed for 1.1:

- `CheckReport`: 5–14;
- `BuildRun` / `BuildFailure`: 1–10;
- `FixReport`: 2–12;
- `EnvironmentComparisonReport`: 1–4.

The 1.0 exit-code contract is unchanged.

## Validation baseline

The feature-complete baseline at commit `6f361ce23e26840fde8e4c5e70d230999393aae1` passed normal CI run `35347314562`:

- source policy;
- Rust 1.85.1;
- Rust stable;
- Docker fixture suite;
- Podman suite;
- real-world smoke corpus.

The Docker fixture log explicitly reported:

`PASS init-output-discovery: temporary build found final ELF without mutating checkout`

Release-hardening changes after that baseline must pass the same normal CI plus the full release-qualification workflow before promotion.

## Release hardening in this candidate

The 1.1 release tree aligns package and lockfile metadata, updates source-policy checks for the 1.1 version, and updates the release workflow to:

- qualify `develop/1.1.0` and `v1.1.*` tags;
- cancel superseded qualification runs on the same ref;
- derive the archive version from `Cargo.toml`;
- verify the binary reports that exact version;
- reject a tag that does not equal `v<package-version>`;
- package `CHANGELOG.md` alongside README/security/license material;
- preserve deterministic tar/gzip/checksum generation.

## Remaining work before stable 1.1

The remaining work is release qualification, not a new product feature layer:

1. pass source-policy and MSRV/stable gates on the candidate commit;
2. pass Docker and Podman synthetic suites;
3. pass the full pinned real-world corpus under both OCI runtimes;
4. inspect the resulting release artifacts and qualification records;
5. verify all version guards and the lockfile agree on `1.1.0`;
6. rerun the complete qualification workflow on the exact final tree;
7. only then create `v1.1.0`.

CI-native user workflows/GitHub Action work remain a 1.2 concern and are intentionally not pulled into this release candidate.
