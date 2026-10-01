# Development baseline — 2026-10-01

## Source identity

- Upstream base: `ee015fa22a261baddb3af2dad75e81783a55f339`.
- Base tree: `2d221fc0e7adc87b4d23fd9212a2ca48c6f80fc9`.
- Development branch: `develop/ci-native-20261001`.
- Separate worktree: `D:/AI_Research/ReproBisect/ReproBisect-dev`.
- Authoritative reviewed plan: `../REPROBISECT_IMPROVEMENT_ROADMAP.md` relative to this document.
- Application source is unchanged from the pinned base. These are documentation additions only.

## Fresh executed checks

Executed through Git Bash on Windows; all three returned exit code 0:

1. `python scripts/static-check.py` — static repository checks passed.
2. `python scripts/real-world-corpus.py validate` — 7 pinned cases validated (4 smoke, 3 extended). This validates the manifest; it does not execute the builds.
3. `python scripts/release-check.py --source-only` — source release policy passed for 1.1.0, including locked package version, schema compatibility policy and 23 pinned external Action references.

Initial direct-Python invocations could not locate Bash. Re-running through the actual Git Bash terminal resolved that environment issue without changing scripts.

## Unmet qualification gates

The following remain blocked, not passed:

- `cargo +1.85.1 check --locked --all-targets`
- `cargo +1.85.1 test --locked --all-targets`
- `cargo +stable check --locked --all-targets`
- `cargo +stable test --locked --all-targets`
- `bash scripts/test-fixtures.sh`
- `bash scripts/test-podman.sh`

Rust/Cargo, Docker and Podman are unavailable in this host shell; WSL reports it is not installed. The Docker fixture entry point stops at missing Cargo. The Podman script additionally encounters an untranslated MSYS `/tmp` path when invoking native Windows Python. These are host limitations, not qualified Linux test failures and not a reason to patch application code.

Real-world execution and full release qualification have not run. Historical upstream successes are not substituted for fresh gates.

## Next action

Run the remaining locked Rust and Docker/Podman gates on a provisioned Linux environment at this exact development commit. Do not mark P00 complete or proceed to dependent implementation before those results are available and reviewed. No push, tag, release, model installation or inference was performed.
