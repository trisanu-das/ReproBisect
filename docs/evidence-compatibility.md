# Persisted evidence compatibility

ReproBisect treats persisted evidence as a versioned interface, not as an implementation detail. Phase 19 introduces a single compatibility reader for the JSON document families emitted by frozen releases and centralizes producer schema numbers in `src/schema.rs`.

## CLI

```text
reprobisect evidence <file>
reprobisect evidence <file> --format json
reprobisect evidence <file> --normalized-output normalized.json
```

Text output reports the detected evidence kind, source schema, current schema, whether migration was applied, how many nested run/failure records were migrated, and any historical semantic caveats. JSON output is the normalized current-schema document itself.

`--normalized-output` never rewrites the source file. The destination is create-only unless `--force` is explicit. Even with `--force`, it may not be the source evidence path and an existing destination symlink is refused. Evidence input is limited to 64 MiB and is read through a hard byte cap in addition to the initial metadata check.

## Supported document families

| Evidence family | Oldest supported | Current in Phase 19 | Historical boundary |
| --- | ---: | ---: | --- |
| `CheckReport` | 5 | 14 | Phase 6 |
| `BuildRun` / `BuildFailure` | 1 | 10 | Phase 6 |
| `FixReport` | 2 | 12 | Phase 6 |
| `EnvironmentComparisonReport` | 1 | 4 | Phase 14 |

The exact frozen release sequence is retained in `tests/evidence-schema-matrix.json`:

| Phase | crate | Check | Build | Fix | Compare |
| ---: | --- | ---: | ---: | ---: | ---: |
| 6 | `0.1.0-alpha.5` | 5 | 1 | 2 | — |
| 7 | `0.1.0-alpha.6` | 6 | 2 | 3 | — |
| 8 | `0.1.0-alpha.7` | 6 | 3 | 4 | — |
| 9 | `0.1.0-alpha.8` | 7 | 4 | 5 | — |
| 10 | `0.1.0-alpha.9` | 8 | 5 | 6 | — |
| 11 | `0.1.0-alpha.10` | 9 | 6 | 7 | — |
| 12 | `0.1.0-alpha.11` | 10 | 7 | 8 | — |
| 13 | `0.1.0-alpha.12` | 11 | 8 | 9 | — |
| 14 | `0.1.0-alpha.13` | 11 | 8 | 9 | 1 |
| 15 | `0.1.0-alpha.14` | 12 | 8 | 10 | 1 |
| 16 | `0.1.0-alpha.15` | 12 | 8 | 10 | 2 |
| 17 | `0.1.0-alpha.16` | 13 | 9 | 11 | 3 |
| 18 | `0.1.0-alpha.17` | 14 | 10 | 12 | 4 |

A document below the minimum is rejected because Phase 19 does not carry enough historical structure to promise a semantics-preserving conversion. A document above the current version is also rejected: a future schema must be understood by newer code rather than silently interpreted using older assumptions.

The compatibility reader also distinguishes an old schema from a malformed document that claims a newer schema. Once a field is mandatory in its declared version, Phase 19 requires it instead of backfilling it. In particular, Build schema 8 must contain `runner_backend`; Build schema 10 must contain the complete Phase-18 log identity fields; and Build schema 4+ cache summaries must contain their bounded/truncation fields. Compatibility migration is not silent corruption repair.

## Migration rules

Migration is performed on a JSON value first and is then deserialized into the current typed Rust model. This makes version checks happen before permissive `serde(default)` behavior can accidentally make an unsupported document look valid.

Nested evidence is migrated recursively:

- control, intervention, stochastic-reference, interaction and confirmation `BuildRun` values;
- structured `BuildFailure` values;
- runs embedded in fix verification reports;
- good/bad/reproduction/confirmation attempts in environment-comparison reports.

After normalization, all migrated top-level and nested evidence carries the current schema number. Fields added with compatibility-safe defaults are materialized by current typed serialization.

### Pre-Phase-18 build logs

Before Build schema 10, stdout/stderr were stored in full as UTF-8 strings and the raw byte-stream SHA-256/length/truncation fields did not exist. Phase 19 derives:

- SHA-256 of the complete persisted string bytes;
- UTF-8 byte length of the persisted string;
- `stdout_truncated = false` / `stderr_truncated = false`;
- `log_capture_max_bytes = 0`, meaning the historical capture bound is unknown/unbounded.

This exactly preserves the text-based failure identity used before Phase 18. It **cannot** recover original non-UTF8 bytes that may have been replaced during the historical lossy UTF-8 conversion, so it is not represented as a reconstructed raw-stream digest.

### Phase-8 dependency-cache summaries

Phase 8 recorded complete, unbounded before/after cache content summaries. Phase 9 introduced explicit truncation and hashing bounds. When a pre-bounded summary is loaded, Phase 19 normalizes it as:

```text
before_truncated = false
after_truncated = false
comparison_complete = true
observed_change = changed_during_build
max_files = 0
max_bytes = 0
```

The zero bounds are sentinels for **unknown historical limits**, not claims that no files or bytes were inspected.

## Non-goals

Compatibility migration does not reinterpret causal conclusions, re-run experiments, strengthen confidence, synthesize missing provenance, or rewrite the original evidence file. It only makes supported historical representations readable through the current typed model while preserving explicit limitations.

The compatibility layer is not a license to keep every future schema forever. v1.0 can declare a longer-term support policy once real-world evidence retention requirements are understood; Phase 19 establishes the mechanism and a concrete Phase-6-forward baseline.
