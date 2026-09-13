# ReproBisect real-world validation corpus

Synthetic fixtures remain the executable specification for individual engine rules. This corpus has a different purpose: exercise the same engine against **pinned, unmodified upstream OSS source trees**.

The corpus does not vendor upstream code. Every case contains:

- a full 40-character Git commit SHA;
- the upstream GitHub repository;
- a ReproBisect configuration overlay;
- an expected top-level result and, where applicable, expected causal variable(s);
- links documenting the upstream release or reproducibility behavior.

`./scripts/real-world-corpus.py prepare` fetches exactly the pinned commit, writes `.reprobisect.toml`, then adds that filename only to `.git/info/exclude`. The upstream working tree therefore remains clean and `HEAD` remains exactly the pinned upstream commit. This matters because many real build systems derive version metadata from Git state.

## Tiers

### `smoke`

Small projects suitable for normal CI:

- cJSON 1.7.18 — debug ELF build-path sensitivity;
- tinyxml2 10.0.0 — debug ELF build-path sensitivity;
- zlib 1.3.1 — release-style build-path negative control;
- byteorder 1.5.0 — Rust debug object build-path sensitivity without network dependencies.

### `extended`

More expensive or specialized cases:

- PyPA sampleproject 2.0.0 with wheel 0.34.2 — the documented wheel/umask reproducibility failure;
- OpenSBI immediately before the upstream reproducibility flag — expected build-path sensitivity;
- the corresponding OpenSBI reproducibility commit with `REPRODUCIBLE=y` — expected fixed control.

The OpenSBI pair is deliberately a before/after test around the real upstream commit that added `-ffile-prefix-map`. It is stronger than inventing an equivalent local patch.

## Commands

```bash
python3 scripts/real-world-corpus.py validate
python3 scripts/real-world-corpus.py list
cargo build
python3 scripts/real-world-corpus.py run --tier smoke \
  --binary target/debug/reprobisect
```

Run one case:

```bash
python3 scripts/real-world-corpus.py run \
  --case cjson-debug-build-path \
  --binary target/debug/reprobisect
```

Use Podman instead of Docker:

```bash
python3 scripts/real-world-corpus.py run --tier smoke \
  --runner podman --binary target/debug/reprobisect
```

Extended cases may build a small helper OCI image first. Helper images are validation infrastructure, not evidence inputs supplied by upstream projects; ReproBisect still resolves the resulting image to an immutable runtime image ID for each experiment.

## Claim boundary

A passing corpus case means ReproBisect produced the case's predeclared result under the pinned source, build recipe, container/toolchain image, and intervention values. It does **not** imply that every release of the upstream project has the same property, or that an upstream project considers the tested configuration a supported release build.

## Result retention

When `--json-output` is supplied, the aggregate result records the SHA-256 of the exact ReproBisect executable, hashes the case/config/helper definitions, and embeds each complete ReproBisect JSON report. CI uploads this artifact even when a corpus case fails, so a regression can be inspected without relying on transient job logs.

## Image pinning boundary

Upstream **source** is pinned by immutable commit SHA and Phase 21 externally supplied build/base images are pinned by OCI `sha256` index digest. Helper images are built locally from those digest-pinned Dockerfiles and use Phase-21-scoped local names. Every executed ReproBisect report additionally records the runtime-resolved image identity.

`real-world-corpus.py validate` rejects a mutable external corpus image or mutable helper Dockerfile base so a later tag move cannot silently alter release qualification.
