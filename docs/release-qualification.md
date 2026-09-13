# ReproBisect 1.0 release qualification

This document defines the qualification contract for the first stable ReproBisect line, `1.0.0`.

A source tree can be internally consistent without having been compiled or exercised against an OCI runtime. ReproBisect therefore separates **source-policy qualification** from **execution qualification** and does not treat a skipped gate as a pass.

## Frozen compatibility contract

The 1.0 stable line freezes these persisted evidence ranges:

| Evidence family | Oldest supported | Current/frozen |
| --- | ---: | ---: |
| `CheckReport` | 5 | 14 |
| `BuildRun` / `BuildFailure` | 1 | 10 |
| `FixReport` | 2 | 12 |
| `EnvironmentComparisonReport` | 1 | 4 |

The compatibility reader added in Phase 19 remains the admission/migration path for historical evidence. A schema change after this point requires an explicit compatibility decision and migration fixture; silently incrementing a producer literal is not an acceptable release change.

The command names `init`, `check`, `diagnose`, `compare`, `fix`, and `evidence`, their default meanings, and the status vocabulary are treated as the 1.0 CLI compatibility surface.

## Qualification gates

The `1.0.0` release commit is qualified only when **all** of the following pass on that exact commit:

1. **Source policy** — `python3 scripts/release-check.py --source-only`, repository static/schema guards, TOML/JSON parsing, shell syntax, immutable corpus inputs, and commit-pinned GitHub Actions.
2. **Compiler/MSRV** — Rust 1.85.x and current stable both pass `cargo check --locked --all-targets` and `cargo test --locked --all-targets`.
3. **Docker synthetic suite** — the full deterministic fixture harness passes under Docker.
4. **Podman synthetic suite** — the Podman acceptance harness passes.
5. **Real-world OSS corpus** — all seven Phase 20/21 cases pass under both Docker and Podman, retaining the aggregate JSON reports as release evidence.
6. **Security/privacy review** — `SECURITY.md` remains accurate; no new host credential/socket mounts, secret persistence, unbounded evidence paths, or silent checkout mutation are introduced.
7. **Packaging** — the release binary reports the exact stable version and the Linux bundle is generated with normalized tar metadata plus `gzip -n`; its SHA-256 is retained.
8. **Stable dependency lock** — `Cargo.lock` is committed from the Rust 1.85.1 release toolchain and every ReproBisect compiler/test/build gate consumes it with `--locked`.

`.github/workflows/release.yml` directly encodes these gates, including both the MSRV and stable Rust compiler lanes, Docker and Podman qualification, the full seven-case corpus, and deterministic Linux packaging. Stable release qualification reruns the complete workflow against the exact `1.0.0` tree with the committed lockfile.

## Supply-chain pinning

Release-qualification infrastructure uses immutable references wherever the project controls the reference:

- GitHub Actions are referenced by full 40-character commit SHA rather than `@v4`/`@stable` aliases.
- External real-world corpus build images use OCI **index digests**, not mutable tags.
- Helper-image Dockerfiles use digest-pinned Debian/Python bases; their locally built helper image names include the Phase 21 validation namespace.
- Upstream OSS source is fetched by full Git commit SHA.

User-supplied `.reprobisect.toml` files may still use image tags. During an experiment ReproBisect resolves the selected image reference to an immutable image identity and records it; requiring users to know registry digests is not part of the CLI contract.

## Exit-code contract

For the 1.0 line:

- `0` — requested operation completed with its success condition (`check` reproducible, `compare` equivalent, or an applicable/verified `fix` result);
- `1` — the operation completed but found a non-success diagnostic condition (for example non-reproducibility, uncontrolled nondeterminism, inconclusive comparison, or no applicable fix);
- `2` — command-line usage/parsing error emitted by Clap;
- `5` — operational/internal error returned through ReproBisect's error path.

JSON output should be preferred by automation. Exit status is deliberately coarse and does not replace the structured report status.

## Qualification evidence

The authoritative release evidence is produced by GitHub Actions against the exact candidate commit. A local or restricted construction environment may perform source-policy checks, but it is not treated as a substitute for the compiler, Docker, Podman, real-world corpus, and packaging jobs encoded in the release workflow.
