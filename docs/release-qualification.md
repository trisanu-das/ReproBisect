# ReproBisect 1.0 release qualification

This document defines the promotion gate for the first stable ReproBisect line. Phase 21 freezes **`1.0.0-rc.1`**, not an unconditional `1.0.0` claim.

A source tree can be internally consistent without having been compiled or exercised against an OCI runtime. ReproBisect therefore separates **source-policy qualification** from **execution qualification** and does not treat a skipped gate as a pass.

## Frozen compatibility contract

The 1.0 release candidate freezes these persisted evidence ranges:

| Evidence family | Oldest supported | Current/frozen |
| --- | ---: | ---: |
| `CheckReport` | 5 | 14 |
| `BuildRun` / `BuildFailure` | 1 | 10 |
| `FixReport` | 2 | 12 |
| `EnvironmentComparisonReport` | 1 | 4 |

The compatibility reader added in Phase 19 remains the admission/migration path for historical evidence. A schema change after this point requires an explicit compatibility decision and migration fixture; silently incrementing a producer literal is not an acceptable release change.

The command names `init`, `check`, `diagnose`, `compare`, `fix`, and `evidence`, their default meanings, and the status vocabulary are treated as the 1.0 CLI compatibility surface.

## Qualification gates

A commit may be promoted from `1.0.0-rc.1` to `1.0.0` only when **all** of the following pass on the exact candidate commit:

1. **Source policy** — `python3 scripts/release-check.py --source-only`, repository static/schema guards, TOML/JSON parsing, shell syntax, immutable corpus inputs, and commit-pinned GitHub Actions.
2. **Compiler/MSRV** — Rust 1.85.x and current stable both pass `cargo check --all-targets` and `cargo test --all-targets`.
3. **Docker synthetic suite** — the full deterministic fixture harness passes under Docker.
4. **Podman synthetic suite** — the Podman acceptance harness passes.
5. **Real-world OSS corpus** — all seven Phase 20/21 cases pass under both Docker and Podman, retaining the aggregate JSON reports as release evidence.
6. **Security/privacy review** — `SECURITY.md` remains accurate; no new host credential/socket mounts, secret persistence, unbounded evidence paths, or silent checkout mutation are introduced.
7. **Packaging** — the release binary reports the exact candidate version and the Linux bundle is generated with normalized tar metadata plus `gzip -n`; its SHA-256 is retained.
8. **Stable dependency lock** — before the final `1.0.0` tag, generate and commit `Cargo.lock` with the release toolchain, rerun the compiler/test gates with `--locked`, and ensure the tree is otherwise unchanged. The RC source deliberately does not claim this gate in environments where Cargo resolution cannot be performed.

`.github/workflows/release.yml` directly encodes gates 1–7 for the release candidate, including both the MSRV and stable Rust compiler lanes. The final stable promotion must additionally switch the package version to `1.0.0`, commit the resolved lockfile, change the release build to `--locked`, and rerun the same workflow.

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

## Current sandbox qualification boundary

The Phase 21 construction sandbox has no usable Rust toolchain, Docker, or Podman, and outbound DNS is unavailable. Source-policy checks can run here; compiler and container gates cannot. This is why the result is a release **candidate**. A future report may call it stable `1.0.0` only after the external workflow evidence exists for the exact promoted tree.
