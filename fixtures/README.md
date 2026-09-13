# ReproBisect fixtures

Fixtures are executable specifications. `.reprobisect.toml` defines the experiment and `fixture.toml` records the expected diagnosis.

CI-gated fixtures:

- `reproducible-c` — stable control project.
- `timestamp-c` — GCC `__DATE__` / `__TIME__` responds to `SOURCE_DATE_EPOCH`.
- `absolute-path-c` — build/workspace path leaks into an ELF.
- `source-path-c` — explicit `{source}` path leaks while build cwd remains fixed.
- `environment-leak` — configured `BUILD_FLAVOR` changes generated output.
- `hostname-embed` — output records the container hostname.
- `timezone-output` — generated output depends on `TZ`.
- `locale-output` — generated output depends on controlled locale.
- `archive-mtime` — source mtime propagates into a TAR member header.
- `umask-archive` — process umask propagates into TAR member mode.
- `parallel-order` — interleaved one-worker references remain stable while concurrent trials produce multiple completion orders; CI requires a supported one-sided Fisher exact stochastic effect at the configured alpha.
- `interaction-two-vars` — neither `A` nor `B` changes output alone; `{A,B}` jointly changes it and must be recovered by interaction/ddmin search.
- `environment-compare` — known-good and known-bad manifests differ in `A`, `B`, and an irrelevant `NOISE` variable; only `{A,B}` jointly reproduces the exact stable bad artifact signature and must be recovered by comparison ddmin.
- `environment-compare-failure` — known-good succeeds while known-bad stably exits 37; only `{A,B}` jointly reproduces the exact exit/stdout/stderr failure signature and `NOISE` must be removed.
- `build-image` — coarse sensitivity to a changed container image.
- `network-required` — disabling networking makes the build command fail and therefore yields scoped hermeticity evidence while byte reproducibility remains inconclusive.
- `toolchain-executable` — changes only the controlled `CC` binding inside one image and requires observed wrapper invocation before high-confidence attribution.
- `toolchain-ar` — changes only `AR`, proves that the archiver wrapper was actually invoked, and changes a static archive deterministically.
- `dependency-lock-variant` — replaces exactly one requirements file in the fresh workspace and records before/after content hashes.
- `provenance-lock` — verifies source/dependency/image/toolchain provenance records.
- `runtime-dependency-provenance` — verifies sanitized post-build resolution state plus before/after aggregate content fingerprints for a configured package cache.
- `runtime-dependency-bounded` — verifies that truncated cache sampling is never promoted to a confirmed mutation.
- `provenance-multi` — exercises normalized/redacted provenance across multiple package-manager lock formats.
- `file-input-provenance` — uses opt-in `strace` provenance to prove one anonymous process successfully read a dependency declaration and configured cache object and wrote the declared output, while the configured private cache-root path remains absent from JSON evidence.
- `file-input-lineage` — passes dependency/cache descriptors into a child, consumes them through `mmap`, writes one declared output directly, and publishes another through a cross-process temporary-file rename; CI requires anonymous parent/child lineage, ancestor input/output correlation, and zero persisted private/temp paths.
- `artifact-formats` — deterministically emits JAR, Python wheel, DEB, OCI image tar, PE/COFF, Mach-O, and WebAssembly outputs and requires type-aware classification plus bounded semantic metadata without external analyzer dependencies.
- `pe-timestamp` — a minimal PE/COFF artifact whose `TimeDateStamp` follows `SOURCE_DATE_EPOCH`; the fixture requires field-level `coff_timestamp` evidence and deterministic-timestamp remediation.
- `log-capture` — emits 2 MiB on each output stream while retaining only 64 KiB per stream; CI requires exact total byte counts, truncation flags, and full-stream SHA-256 values.
- `fixable-path-c` — path-sensitive ELF whose root Makefile receives an append-only temporary prefix-map patch candidate during verification.
- `timestamp-c` — also exercises temporary Makefile patch verification for `SOURCE_DATE_EPOCH`; the original fixture Makefile must remain unchanged.
- `fixable-path-cargo` — path-sensitive Rust binary whose fix candidate creates `.cargo/config.toml` only in the temporary verification checkout.
- `fixable-archive-mtime-make` — verifies a temporary GNU-tar timestamp-normalization Makefile patch.
- `fixable-umask-make` — verifies a temporary GNU-tar mode-normalization Makefile patch.
- `fixable-archive-mtime-shell` — verifies a one-site scoped `TAR_OPTIONS` replacement in a detected packaging shell script without mutating the original checkout.

Best-effort/non-gated fixture:

- `directory-order` — intentionally depends on inode/materialization order. Filesystem enumeration and inode allocation differ by host/filesystem, so this fixture documents the dimension but is not a portable CI assertion.

The fixtures intentionally separate *detection* from *causal attribution*. A path-containing ELF, for example, is diagnosed only when the controlled path intervention changes artifact bytes; strong confidence additionally requires reversion and direct/structural evidence.
