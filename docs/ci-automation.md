# CI automation envelope v1

This development interface is opt-in; it is not a claim that a release has been published.

```sh
reprobisect check . --ci > ci.json
reprobisect diagnose . --ci --ci-policy report-only > ci.json
```

`--ci` produces one JSON document on stdout for a completed check and handled
usage/operational failures when stdout is writable. Diagnostics use best-effort,
non-panicking stderr writes; stderr failure cannot suppress the envelope or change
its usage/operational exit. Diagnostic text omits control characters and is capped
at 4,096 Unicode characters per message, including any error prefix/underlying
error chain (plus a terminating newline).
It is non-interactive, emits no progress/spinner, and overrides `--format` and
`--verbose` output selection. `--ci-policy` requires `--ci`. Help/version still
use the normal CLI presentation. Any CI stdout write/flush failure exits 5,
even if the original error was usage (2). A closed/unwritable output pipe cannot
receive an envelope; a partial write may leave truncated JSON. No replacement
JSON document is appended after an emission failure, even if the transport
recovers. Consumers must reject failed/partial output, not treat it as approval.
Termination/cancellation/total execution budgets belong to P02;
P01 does not add a cancellation exit code or partial-report recovery.

The schema is [`../schemas/automation-v1.schema.json`](../schemas/automation-v1.schema.json).
Consumers should require `schema_version == 1` and inspect both diagnostic truth
and job policy, not infer reproducibility from the process exit alone.

## Policy and exit contract

| Outcome | Default `require-reproducible` | `report-only` |
| --- | --- | --- |
| Complete reproducible check | 0 | 0 |
| Complete non-reproducible/inconclusive check | 1 | 0 |
| Incomplete coverage, including unstable baseline | 1 | 1 |
| Usage error | 2 | 2 |
| Operational/evidence-validation error | 5 | 5 |

`diagnostic_status` preserves the engine's existing `CheckStatus`. A policy
never rewrites it. `policy.diagnostic_exit_code` is the legacy check exit for a
returned report; for usage/operational failures it records 2/5 with a null status.
`policy.exit_code` is the actual job-policy exit; `policy.passed` is never true
on missing evidence, skipped mandatory work, or failed execution. Invalid CLI
arguments use the default policy name because no parsed policy was established.
`report-only` permits a completed finding; it is **not** a reproducibility proof.

## Completion and scope

`completion.completed` means all work required by the configured scope was
observed without execution gaps, not that the artifacts are reproducible.
Reasons are `completed`, `incomplete_coverage`, `usage_error`, and
`operational_error`. A report may exist even when completion is false.

`coverage.controls` counts control builds. `coverage.interventions` counts
configured intervention IDs, not variable names or total container launches:
multiple image variants are distinct candidates. `attempted` counts IDs with
returned results; `completed` additionally requires all planned variant trials,
matched reference trials for CPU interventions, conditional reversal trials,
and no recorded execution error. Counts distinguish `skipped` from
`failed` (attempted but incomplete). Structured nonzero build outcomes are
observations for dimensions that support them, not fabricated artifact deltas.
The trial-count rule is shared with the engine, including minimum two trials
for network/toolchain/dependency interventions.

Unstable controls can return before the engine creates the intervention list.
The envelope independently derives the configured plan and reports skipped work;
it does not interpret an empty result list as a completed zero-candidate run.

Interaction coverage identifies disabled/not-applicable work, unstable-baseline
blocking, candidate-cap skips, completed searches, and failed/missing searches.
Candidate selection shares the engine's exact predicate: an admissible planned
intervention whose individual result is unchanged and has no execution error.
Changed single-variable findings are outside that set and cannot inflate its cap.
Its strategy is `combined_candidate_set_then_ddmin_not_exhaustive`: the engine
probes its admissible combined candidate set and minimizes a stable effect;
it does **not** exhaustively test all interactions. `tested_subsets` is the
engine's recorded strategy count, not a reconstructed count of every trial.
When errors leave no structured interaction result, `failed_or_missing` is
honest uncertainty; free-form error notes are not parsed into causal findings.

## Identity and immutable evidence

- `binary_version` is the compiled package version; `binary_sha256` hashes the
  running executable. It identifies bytes, not a claimed reproducible build.
- `effective_config_sha256` hashes canonical JSON of the parsed/defaulted
  `Config` after normalizing effective control/intervention/confirmation/stochastic/
  interaction settings into it. Overridden original values are not retained:
  equivalent effective settings supplied through TOML or CLI have the same identity.
  It is **not** a hash of TOML formatting or a source-commit substitute.
- `source_sha256` and `experiment_id` come from the authoritative engine report.
- `report.path` is project-relative. Its SHA-256 hashes the exact persisted
  JSON bytes, including newline; those bytes must match the returned report.
  Missing, unreadable, or mismatched evidence is operational failure under both policies.
- On errors before a validated report is available, identities not supplied by
  the completed pipeline, coverage and the report reference are null, not zero.

The payload omits build commands, environment values, raw build logs, and free-form
engine notes. Stderr is local diagnostic output, not a promised redaction system;
P03 governs broader disclosure/privacy controls. Hashes are identities, not signatures.
No hosted model, credential, model download, or extra runtime dependency is required.

## Compatibility and verification

Without `--ci`, text/legacy `--format json` behavior and 0/1/2/5 exits remain
unchanged. Existing persisted BuildRun, CheckReport, comparison and fix schemas
remain unchanged, as do historical evidence fixtures and normalization.
The automation envelope is a separate versioned presentation schema.

Run ordinary portable tests with `cargo test --locked --all-targets`.
Linux with Docker additionally runs:

```sh
cargo test --locked --test cli ci_live_ -- --ignored
```

These subprocess tests exercise both policies/operations, reproducible/finding
and unstable-control outcomes, immutable report hashes, binary identity,
legacy JSON exits, and stdout cleanliness through the actual container pipeline.
