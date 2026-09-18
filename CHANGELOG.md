# Changelog

## 1.1.0-rc.1 — Release candidate

- adds `reprobisect doctor` to validate project configuration and OCI runtime readiness before starting expensive rebuild experiments;
- upgrades `reprobisect init` with deterministic build-system detection and editable image/command/output suggestions for Cargo, Go, npm/pnpm, Maven, Gradle, Bazel, Meson, CMake, Autotools, Python packaging, and Make projects;
- reports init confidence and multi-marker ambiguity explicitly, while retaining low-confidence placeholders when a concrete artifact cannot be inferred safely;
- adds opt-in `reprobisect init --discover-outputs`, which runs one detected build in a temporary OCI workspace, diffs produced files, filters common intermediates, ranks executable/archive/package candidates, and writes only high-confidence candidates to `build.outputs` while leaving weaker candidates advisory; `--runner docker|podman` controls both probing and the generated config;
- redesigns the default text report around the diagnosis: result, cause, confidence, affected artifacts, baseline/variant/reversion hashes, evidence, remediation, and a compact tested-variable effect summary; full experiment/provenance detail remains available with `-v`;
- provides text and JSON doctor reports with explicit PASS/WARN/FAIL/SKIP checks;
- treats a missing local build image as a warning while treating an unavailable runner/runtime as not ready;
- keeps persisted evidence schema versions and the 1.0 exit contract unchanged;
- aligns package/lockfile identity with `1.1.0-rc.1` and hardens release qualification so artifact versioning is derived from `Cargo.toml`, tag/version mismatch is rejected, and deterministic packaging runs only after source, compiler, Docker, Podman, and real-world corpus gates;
- leaves the published `v1.0.0` release as the supported stable line until final 1.1 promotion.

## 1.0.0 — Stable

First stable ReproBisect release.

- retains the Phase 21 persisted-evidence schema freeze: CheckReport 14, BuildRun/BuildFailure 10, FixReport 12, and EnvironmentComparisonReport 4;
- commits the Cargo dependency lock generated and checked with Rust 1.85.1;
- enforces `--locked` across CI, fixture, real-world corpus, release-build, and release-policy compiler/test gates;
- qualifies the same seven-case real-world OSS corpus under both Docker and Podman before packaging;
- packages the Linux release deterministically only after source policy, MSRV/stable Rust, Docker, and Podman qualification succeed.

## 1.0.0-rc.1 — Phase 21

Release-hardening candidate for the first stable line.

- freezes the persisted evidence schemas at CheckReport 14, BuildRun/BuildFailure 10, FixReport 12, and EnvironmentComparisonReport 4;
- fixes a Phase 20 CLI compile blocker in verbose check output;
- pins real-world corpus OCI inputs by immutable digest;
- pins GitHub Actions dependencies by exact commit;
- adds an MSRV/stable compiler matrix and explicit release qualification workflow;
- adds security/privacy guidance and a machine-checkable release policy;
- adds deterministic Linux release-bundle packaging after all qualification jobs pass;
- hands rootful Docker umask experiment outputs back to the host owner without changing artifact modes or bytes, avoiding false `inconclusive` results from unreadable bind-mounted files;
- corrects the byteorder Rust smoke-case OCI pin to the official Rust toolchain image so `rustc` is present during qualification.

Promotion from this release candidate to `1.0.0` requires the full compiler, Docker, Podman, synthetic-fixture, and seven-case real-world qualification gates described in `docs/release-qualification.md`.

## 0.1.0-alpha.19 — Phase 20

Added the pinned real-world OSS validation corpus and CI harness.

## 0.1.0-alpha.18 — Phase 19

Added persisted evidence compatibility and historical schema migration.
