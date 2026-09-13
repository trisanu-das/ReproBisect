# Changelog

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
