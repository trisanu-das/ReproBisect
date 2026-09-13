# Real-world OSS validation

Phase 20 introduced a validation layer that is deliberately separate from the synthetic fixture suite.

Synthetic fixtures are small executable specifications for individual engine rules. The real-world corpus asks a different question: does the same ReproBisect engine produce the predeclared result when pointed at an exact, unmodified upstream OSS revision and a realistic build recipe?

## Corpus contract

Every case under `corpus/real-world/` contains a schema-versioned `case.toml` plus a ReproBisect config overlay. A case must declare:

- an HTTPS GitHub repository URL;
- a full lowercase 40-character commit SHA;
- an upstream tag/ref description;
- a smoke or extended tier;
- a bounded case timeout;
- an expected top-level ReproBisect status;
- expected causal variables for non-reproducible cases;
- at least one upstream or reproducibility reference;
- an explicit Docker/Podman runner in its ReproBisect config.

The source itself is not vendored. `scripts/real-world-corpus.py` initializes an empty Git repository, fetches exactly the pinned commit with `--depth=1 --no-tags`, checks out `FETCH_HEAD`, and requires `HEAD` to equal the manifest SHA exactly.

The harness writes only `.reprobisect.toml` into the source checkout and excludes that file and `.reprobisect/` through `.git/info/exclude`. It then requires `git status --porcelain` to remain empty. This preserves the exact upstream commit and avoids changing build/version behavior that depends on Git dirty state.

## Phase 20/21 cases

| Case | Tier | Upstream pin | Expected result | Controlled dimension |
| --- | --- | --- | --- | --- |
| `cjson-debug-build-path` | smoke | cJSON `acc76239bee01d8e9c858ae2cab296704e52d916` (`v1.7.18`) | non-reproducible | build path |
| `tinyxml2-debug-build-path` | smoke | tinyxml2 `321ea883b7190d4e85cae5512a12e5eaa8f8731f` (`10.0.0`) | non-reproducible | build path |
| `zlib-release-path-control` | smoke | zlib `51b7f2abdade71cd9bb0e7a373ef2610ec6f9daf` (`v1.3.1`) | reproducible | build path negative control |
| `byteorder-rustc-build-path` | smoke | byteorder `ec068eefa042d494475db125c4b034bd8e9e34dd` (`1.5.0`) | non-reproducible | build path |
| `sampleproject-wheel-umask` | extended | PyPA sampleproject `3b0207a3396b634e41af20e82e0c710194d5f5dd` (`2.0.0`) | non-reproducible | umask |
| `opensbi-pre-prefix-map` | extended | OpenSBI `66ab965e54017292d530033ffaae127389c45458` | non-reproducible | build path |
| `opensbi-post-prefix-map` | extended | OpenSBI `6d23a9c5707de7243c0b3423724c0e4f3e1ee40d` | reproducible | build path negative control |

The OpenSBI cases intentionally straddle the upstream commit that introduced a `REPRODUCIBLE=y` path-remapping flag. The post-fix case enables that upstream switch; ReproBisect does not inject its own prefix-map fix into the control.

The wheel case pins the older `wheel==0.34.2` behavior and fixes `SOURCE_DATE_EPOCH`, leaving umask as the selected intervention. It exists to exercise a historically documented permission-mode reproducibility failure rather than a ReproBisect-created toy package.

## Running the corpus

Offline manifest/config validation:

```bash
python3 scripts/real-world-corpus.py validate
python3 scripts/real-world-corpus.py list
```

Normal smoke run:

```bash
cargo build
python3 scripts/real-world-corpus.py run \
  --tier smoke \
  --binary target/debug/reprobisect \
  --json-output real-world-smoke.json
```

Select individual cases by repeating `--case`. Use `--runner podman` to render each overlay for Podman. A supplied `--work-root` is never overwritten implicitly; `--replace-checkout` is required to remove a pre-existing per-case checkout.

Extended cases can declare a small helper Dockerfile and image tag. The harness requires the helper image name to be exactly the config's build image and builds it through the selected OCI runtime unless `--no-helper-build` is requested.

## Result artifact

The aggregate JSON result is intended to be retained by CI. It contains:

- corpus result schema version;
- selected OCI runner;
- SHA-256 of the exact ReproBisect executable;
- exact upstream repository and commit for every case;
- SHA-256 of each case manifest/config and helper Dockerfile when present;
- expected and observed status;
- expected/observed causal variables;
- duration and exit code;
- the complete ReproBisect JSON report for each successful harness execution.

The complete report is retained because a pass/fail-only corpus would hide confidence, image identity, artifact hashes, intervention trials, and limitations that are needed when diagnosing a regression in ReproBisect itself.

## CI tiers

`.github/workflows/ci.yml` runs the four smoke cases with Docker on ordinary push/PR CI and uploads the aggregate result even when the corpus job fails.

`.github/workflows/real-world.yml` is an explicit extended-corpus workflow. It can run with Docker or Podman and uploads the extended result artifact. The split keeps normal feedback bounded while preserving a path to the heavier cross-toolchain cases.

## Reproducibility and claim boundary

The **source revisions and externally supplied OCI inputs are immutable pins** in Phase 21. cJSON/tinyxml2/zlib use the same digest-pinned GCC index; byteorder uses a digest-pinned Rust 1.85 image; the OpenSBI and wheel helper Dockerfiles start from digest-pinned Debian/Python indexes. Locally built helper-image tags are Phase-21-scoped and their Dockerfile hashes are retained in corpus output.

ReproBisect still records the runtime-resolved image identity in each run, giving a second execution-time check on the corpus definition. The digest pins freeze registry content while the retained case/config/helper hashes freeze the validation recipe.

A passing case establishes only the predeclared observation under that exact source revision, recipe, resolved image, intervention values, and ReproBisect executable. It does not establish universal reproducibility for the upstream project, and the corpus does not modify upstream source to manufacture expected results.
