# ReproBisect implementation status — Phase 21

**Status date:** 2026-09-13
**Release target:** `1.0.0-rc.1`
**Scope:** v1.0 release hardening, frozen CLI/schema contract, immutable validation inputs, supply-chain hardening, and explicit release qualification

## Summary

Phase 21 converts the Phase 20 implementation into the first stable-line release candidate. It deliberately does **not** claim final `1.0.0`: promotion is gated on compiler/MSRV checks, Docker and Podman execution, the complete real-world corpus, a committed dependency lock, and deterministic release packaging on the exact candidate commit.

No persisted evidence schema changes are introduced in this phase. The compatible schema ranges remain frozen while the release process, CLI exit semantics, CI policy, security boundary, and validation inputs are hardened around them.

## Release-candidate contract

The crate version is `1.0.0-rc.1` with Rust MSRV `1.85` and repository toolchain `1.85.1`.

The user-facing command set is frozen for the 1.0 line:

- `init`;
- `check` / `diagnose`;
- `compare`;
- `fix`;
- `evidence`.

The process exit contract is explicit:

- `0` — operation completed without a diagnostic/finding status;
- `1` — a reproducibility finding, minimized difference, uncontrolled nondeterminism, inconclusive result, or unsuccessful fix result;
- `2` — Clap command-line usage error;
- `5` — operational/internal error returned through the top-level `anyhow` boundary.

Phase 21 introduces named Rust constants for the ReproBisect-owned exit values so accidental numeric drift is statically guarded.

## Source-level defect fixed during release audit

The Phase 20 source contained a real compile blocker in `src/cli.rs`: `run_check` attempted to print text using `options.verbose`, although `CheckOptions` intentionally has no `verbose` member. Phase 21 fixes the call to use `args.verbose` and adds a static regression guard specifically for this failure class.

This discovery is also why Phase 21 remains conservative about qualification claims: text/static checks are useful but are not substitutes for a real Rust compiler gate.

## Immutable real-world qualification inputs

The Phase 20 seven-project real-world corpus remains the empirical validation set, but its OCI inputs are hardened for release qualification.

External build images are now referenced by immutable `@sha256:` OCI index digests rather than mutable tags. Helper images use local Phase 21 names whose Dockerfile `FROM` inputs are themselves digest-pinned. Offline corpus validation rejects external mutable image references and rejects helper Dockerfiles whose base image is not digest-pinned.

The corpus continues to pin every upstream project to a full 40-character Git commit. Result artifacts retain the exact ReproBisect executable SHA-256, upstream commit, case/config/helper/validator hashes, timings, expectations, outcomes, and full ReproBisect JSON report.

## CI and release workflow hardening

GitHub Actions dependencies are pinned to exact 40-character action commits instead of mutable major-version tags or branches.

Normal CI now separates:

1. source-policy/static validation;
2. Rust compiler checks on MSRV `1.85.1` and stable;
3. synthetic Docker fixture qualification;
4. synthetic Podman fixture qualification;
5. Docker real-world smoke corpus.

The extended corpus workflow remains manually dispatchable for Docker or Podman.

A new release workflow gates package creation on Rust/source qualification plus a Docker/Podman container matrix. It runs the synthetic acceptance suite and the complete seven-case corpus before generating the Linux release archive.

Release packaging uses a normalized tar stream (`--sort=name`, epoch mtime, numeric uid/gid 0) and `gzip -n` before producing SHA-256 checksums.

## Security and release documentation

Phase 21 adds:

- `SECURITY.md` — threat model, secret/logging boundary, container assumptions, privacy guarantees and reporting guidance;
- `CHANGELOG.md` — stable-line release-candidate history;
- `docs/release-qualification.md` — exact promotion gates and schema/CLI compatibility boundary;
- `scripts/release-check.py` — machine-readable source/release policy verification.

The security model explicitly states that a build command is untrusted code executed through the selected OCI runtime; ReproBisect is not itself a hardened hostile-code sandbox.

## Evidence compatibility freeze

Persisted evidence schemas remain unchanged from Phase 20:

- `CheckReport`: current **14**, supported **5–14**;
- `BuildRun` / `BuildFailure`: current **10**, supported **1–10**;
- `FixReport`: current **12**, supported **2–12**;
- `EnvironmentComparisonReport`: current **4**, supported **1–4**.

The evidence schema history matrix now covers frozen Phases 6 through 21 and asserts that the Phase 19, 20 and 21 release lines retain those current schema values.

## Validation performed in this environment

The Phase 21 source-policy gate passes in this sandbox. It covers:

- `scripts/static-check.py`;
- complete repository TOML parsing;
- historical evidence/schema-matrix checks;
- offline seven-case corpus validation;
- GitHub Actions YAML parsing;
- pinned-action policy checks;
- shell syntax for Docker and Podman fixture harnesses;
- Python syntax for corpus/release scripts;
- `git diff --check`;
- release metadata/version/MSRV/schema policy checks through `scripts/release-check.py --source-only`.

The frozen Phase 20 source archive was independently re-materialized as a Git tree before Phase 21 work; its tree matched the published Phase 20 tree exactly.

## Qualification not available in this sandbox

This environment does not provide `rustc`, Cargo, Docker, or Podman, and outbound shell DNS is unavailable. Attempts to reach Debian package infrastructure fail at DNS resolution. Therefore this phase does **not** claim execution of:

- `cargo check --all-targets`;
- `cargo test --all-targets`;
- MSRV/stable compiler matrix;
- Docker synthetic fixtures;
- Podman synthetic fixtures;
- the seven real-world builds under either OCI backend.

Those are mandatory release-candidate qualification gates, not optional checks. A skipped gate is not counted as a pass.

## Remaining gate before final `1.0.0`

`1.0.0-rc.1` is the end of the planned implementation phases, but not yet the final stable release claim. Promotion to `1.0.0` requires all gates in `docs/release-qualification.md` to pass on the exact candidate tree.

In addition, the final stable tree must commit a `Cargo.lock` generated by the qualifying Rust toolchain and rerun Rust/release builds with `--locked`. This sandbox cannot truthfully generate or validate that lockfile because it has no Cargo and cannot fetch the dependency index.

If those external gates expose defects, they should be fixed as release-candidate corrections rather than silently waived.
