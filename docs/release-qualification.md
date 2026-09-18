# ReproBisect 1.1 release qualification

This document defines the qualification contract for the ReproBisect 1.1 line. The current package version on `develop/1.1.0` is `1.1.0`. This is the final source version under qualification; it is not a public stable release until tag `v1.1.0` is created after all gates pass on the exact final tree.

A source tree can be internally consistent without having been compiled or exercised against an OCI runtime. ReproBisect therefore separates **source-policy qualification** from **execution qualification** and does not treat a skipped gate as a pass.

## Compatibility contract

ReproBisect 1.1 intentionally does **not** change persisted evidence schema versions from 1.0:

| Evidence family | Oldest supported | Current |
| --- | ---: | ---: |
| `CheckReport` | 5 | 14 |
| `BuildRun` / `BuildFailure` | 1 | 10 |
| `FixReport` | 2 | 12 |
| `EnvironmentComparisonReport` | 1 | 4 |

Historical evidence remains admitted through the compatibility reader introduced before 1.0. Any future persisted-schema change requires an explicit compatibility decision and migration fixture; a version bump must not be hidden inside a feature release.

The existing command semantics for `check`, `diagnose`, `compare`, `fix`, and `evidence` remain compatible. 1.1 adds adoption/diagnostic surface around them:

- `doctor` readiness checks in text and JSON;
- build-system-aware `init`;
- explicit init confidence and ambiguity reporting;
- opt-in `init --discover-outputs`;
- `init --runner docker|podman`;
- diagnosis-first default text reporting with full detail retained under `-v`.

## Qualification gates

A 1.1 candidate is qualified only when **all** of the following pass on the exact commit being considered:

1. **Source policy** — `python3 scripts/release-check.py --source-only`, repository static/schema guards, TOML/JSON parsing, shell/Python syntax, immutable corpus inputs, committed lockfile, and commit-pinned GitHub Actions.
2. **Compiler/MSRV** — Rust 1.85.1 and current stable both pass `cargo check --locked --all-targets` and `cargo test --locked --all-targets`.
3. **Docker synthetic suite** — the complete deterministic fixture harness passes. This includes the 1.1 output-discovery regression that must select the final ELF, exclude an object-file intermediate, and leave the original checkout unbuilt.
4. **Podman synthetic suite** — the Podman acceptance harness passes.
5. **Real-world OSS corpus** — all pinned corpus cases pass under both Docker and Podman, retaining aggregate JSON reports as release evidence.
6. **Security/privacy review** — `SECURITY.md` remains accurate; no host credential/socket mounts, unbounded persisted evidence, or silent checkout mutation are introduced. Output discovery must retain its bounded scan/log/runtime behavior.
7. **Packaging** — the release binary reports the exact package version; a tag build must use tag `v<package-version>`; the Linux bundle is generated with normalized tar metadata plus `gzip -n`; its SHA-256 is retained.
8. **Dependency lock** — `Cargo.lock` matches the package version and every compiler/test/build gate consumes it with `--locked`.
9. **1.0 compatibility regression** — existing evidence fixtures and established 1.0 experiment/fix behavior continue to pass; 1.1 adoption features do not weaken causal-promotion or reversion requirements.

`.github/workflows/release.yml` encodes the compiler matrix, Docker/Podman qualification, full real-world corpus, exact binary-version check, tag/package-version agreement, and deterministic Linux packaging. The workflow runs on `develop/1.1.0`, may be dispatched manually, and accepts only `v1.1.*` tags.

## Supply-chain pinning

Release-qualification infrastructure continues to use immutable references wherever the project controls the reference:

- GitHub Actions are referenced by full 40-character commit SHA;
- external corpus build images use OCI digest references;
- helper-image bases are digest-pinned;
- upstream OSS source is fetched by full Git commit SHA.

User-supplied `.reprobisect.toml` files may still use image tags. During an experiment ReproBisect resolves the selected image reference to an immutable image identity and records it.

## Exit-code contract

The stable 1.0 exit contract remains unchanged in 1.1:

- `0` — requested operation completed with its success condition;
- `1` — operation completed but produced a reproducibility/diagnostic finding or other non-success result;
- `2` — command-line usage/parsing error emitted by Clap;
- `5` — operational/internal error returned through ReproBisect's error path.

JSON output remains the preferred automation contract; exit status is deliberately coarse.

## Final `1.1.0` publication gate

The source version has already been promoted from `1.1.0-rc.1` to `1.1.0`. Publication now requires:

1. all gates above passing again on the exact final `1.1.0` tree;
2. no known high-severity regression or false-positive class introduced by the 1.1 changes;
3. `Cargo.toml`, the root `Cargo.lock` package entry, `scripts/release-check.py`, and `scripts/static-check.py` all agreeing on `1.1.0`;
4. retaining the release workflow's exact binary/package-version and tag/package-version checks;
5. creating tag `v1.1.0` only after those gates pass.

The public `v1.0.0` release remains the supported stable release until that tag/release is created.
