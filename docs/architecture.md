# v0.1 Architecture Notes

The codebase remains one Rust crate until internal APIs stabilize.

## Execution path

```text
CLI
 └─ Config loader
    └─ Experiment controller
       ├─ deterministic source snapshot digest
       ├─ canonical ControlledEnvironment
       ├─ OCI runner (Docker or Podman)
       │  ├─ immutable image-ID resolution per tested image
       │  ├─ best-effort toolchain probes per image ID
       │  ├─ stable controlled-tool wrappers + invocation counters
       │  ├─ fresh source materialization
       │  ├─ isolated /reprobisect-meta instrumentation mount
       │  ├─ /src source alias
       │  └─ /workspace build/cwd alias
       ├─ repeated control gate
       ├─ source/dependency provenance capture
       ├─ optional pre/post dependency-cache content summaries
       ├─ one-variable intervention planner
       │  ├─ coarse image variants
       │  ├─ narrow toolchain executable bindings
       │  ├─ one-file dependency declaration replacements
       │  └─ network-off build-viability experiments
       ├─ artifact collector/analyzers
       │  ├─ SHA-256
       │  ├─ controlled marker scan
       │  ├─ ELF section/build-ID analysis
       │  └─ archive metadata analysis
       ├─ reversion confirmation
       ├─ optional interaction/ddmin search
       ├─ diagnosis synthesis
       ├─ optional rule-based fix verification
       └─ immutable report/evidence persistence
```

## Separation of responsibilities

The runner is intentionally ignorant of causal meaning. It accepts a complete `ControlledEnvironment` and executes it. `run_attempt` distinguishes a build command that started and exited non-zero from OCI-runtime/orchestration errors, which allows network-hermeticity experiments to retain build failure as an observable without misclassifying infrastructure failure. The intervention planner decides *what changes*; the controller records *which experiments and repetitions were observed*; analyzers produce bounded artifact evidence; the diagnosis layer decides *what can be claimed*.

This separation keeps OCI-runtime behavior out of the causal model. The current local runner supports Docker and Podman through one execution path; Nix/remote runners remain future backends.

## Controlled baseline

The canonical baseline currently fixes:

- source alias: `/src`,
- build/cwd alias: `/workspace`,
- source materialization order: sorted lexical order,
- `SOURCE_DATE_EPOCH=946684800`,
- `TZ=UTC`,
- `LANG=C`,
- `LC_ALL=C`,
- hostname: `reprobisect-control`,
- configured environment-variable baselines,
- source mtime only when that experiment is enabled,
- umask only when that experiment is enabled,
- CPU quota/count only when that experiment is enabled.

This is an experimental reference environment, not a prescription for production builds.

## Provenance, image interventions, and network availability

Before experimentation, the controller records the source-tree digest, containing Git commit/dirty state when available, and SHA-256 fingerprints for common dependency lock/manifest files. These records establish what declaration snapshot was tested; they do not prove package-manager resolution was locked.

A `BuildImage` intervention changes `ControlledEnvironment.image_override`. Each requested image reference is resolved once to an immutable Docker image ID, and a best-effort toolchain probe is cached by that ID. Because an image bundles many variables, diagnosis confidence is capped at medium even when the effect is stable and baseline reversion succeeds.

A `NetworkAccess` intervention changes `network_mode` from `default` to `none`. Its run path uses structured outcomes:

```text
Success(BuildRun)         -> compare artifact bytes
BuildFailed(BuildFailure) -> causal build-viability evidence
Err(...)                   -> infrastructure error / inconclusive experiment
```

This deliberately avoids treating Docker failure as evidence that the project requires a network.

A `ToolchainExecutable` intervention changes one explicit binding such as `CC`, `LD`, or `AR` while retaining the same resolved image ID. The build sees a stable wrapper path under `/reprobisect-meta/toolchain-wrappers`; only the wrapper's delegated executable changes. Each wrapper records one marker per invocation and never records arguments. The runner separately probes the delegated executable's resolved path/version. A high-confidence toolchain diagnosis therefore requires observed invocations on both sides in addition to stable artifact change and reversion. If the binding was never invoked, ReproBisect reports sensitivity to the binding value at low confidence rather than claiming executable causality.

A `DependencyFile` intervention replaces exactly one configured project-relative regular file inside the fresh workspace. Before replacement, the runner hashes both target and variant; those hashes and paths are retained as causal evidence. The original checkout is never modified.

Optional network tracing, controlled-tool probing, invocation logs, and dependency-cache summaries write only to a separate host temporary directory bind-mounted at `/reprobisect-meta`. Instrumentation files are therefore not visible under `/src` or `/workspace`, preventing `find`, globbing, archive traversal, or directory-order-sensitive builds from observing ReproBisect's own trace files. Raw network traces and temporary cache-member hash lists are destroyed with that directory. Persisted network evidence is reduced to syscall counts, connect success/failure counts, address families, and endpoint-scope classes; persisted cache evidence is reduced to before/after counts, byte totals, and aggregate content digests. Neither mechanism proves where a byte originated.

## Runner output capture and resource ceilings

OCI runtime stdout and stderr are drained concurrently on dedicated reader threads. Each stream is incrementally SHA-256 hashed and byte-counted to EOF, but only a bounded raw prefix is retained for conversion to persisted text. The default retention budget is 1 MiB per stream; configuration validation permits 64 KiB through 16 MiB. This prevents `wait_with_output`-style unbounded in-memory buffering while preserving an exact digest of the complete stream.

`BuildRun` and `BuildFailure` persist the retention budget, total bytes, full-stream SHA-256, and truncation state. Failure-signature comparison prefers these full-stream digests, falling back to the stored text only for older evidence that predates the digest fields.

Experiment repetition counts are validated as resource budgets as well as statistical parameters. Deterministic control/intervention/interaction/comparison dimensions are capped at 32 runs; stochastic CPU-count experiments permit 3–1024 matched pairs. These limits bound accidental experiment explosion without changing the causal requirement that each promoted result satisfy its configured repetition policy.

## Source path versus build path

The same fresh host workspace is bind-mounted at two distinct container aliases. The command runs under the build alias; the source alias is independently addressable through `{source}`. This gives a generic in-source build backward-compatible behavior while allowing explicit source-path tests:

```text
host fresh workspace
   ├─ bind → /src
   └─ bind → /workspace  (cwd)
```

`SourcePath` changes only `/src`; `BuildPath` changes only `/workspace`. Because both aliases point at the same fresh snapshot, source bytes are held constant while path spelling changes.

This is not a full out-of-tree build graph abstraction; it is a controlled aliasing mechanism suitable for v0.1 causal path experiments.

## Source materialization order

`SourceCopyOrder::Sorted` is the baseline. The opt-in directory-order intervention uses `Reverse`, changing the sequence in which source entries are created in the fresh workspace while preserving source bytes and logical paths.

This is explicitly **best effort**. Filesystems may expose directory entries in hash order or another representation unrelated to creation order. Therefore a positive effect is useful causal evidence, but a negative result cannot prove traversal-order independence.

## Stable controls and stochastic variants

Strong deterministic attribution is allowed only after repeated control builds produce identical declared artifacts.

CPU-count experiments are special. After the initial stable-control gate, the controller executes `stochastic_runs` interleaved baseline→variant pairs. Every trial is reduced to whether its declared-artifact signature differs from the original stable baseline signature. The statistics layer applies a fixed-sample, one-sided Fisher exact test to the resulting 2×2 table and persists the observed rates, absolute rate difference, p-value, configured alpha, and classification. Multiple distinct variant artifacts are still retained rather than collapsed into a representative value.

The extra baseline references are also a second stability gate. If any reference changes, the initial finite control sample was insufficient: the entire check is reclassified as `UNCONTROLLED_NONDETERMINISM`, diagnoses are suppressed, and interaction search is not run. This is intentionally stricter than treating baseline noise as a nuisance term.

## Interaction search

Interaction search is optional and uses only one-variable experiments that were individually inert and completed successfully. The controller:

1. applies all eligible inert interventions together,
2. requires a stable joint failure before minimization,
3. uses ddmin to find a 1-minimal observed subset,
4. reruns the minimized subset,
5. reverts to baseline before promoting the interaction diagnosis.

The result is scoped to the candidate variables and tested build configuration.

## Artifact evidence

SHA-256 equality is authoritative for v0.1.

Additional evidence is bounded and typed:

- exact controlled-value byte markers,
- ELF marker locations with section names,
- GNU ELF build ID,
- PE/COFF, Mach-O, and WebAssembly header classification,
- JAR, Python wheel, DEB, and OCI-image structural classification,
- bounded semantic summaries for selected package/container metadata and executable headers,
- archive/package member metadata and order deltas,
- semantic-field deltas carried into intervention evidence.

Artifact evidence alone never establishes causality. It strengthens a causal claim only after a controlled intervention changes the artifact.

## Fix verification

The fix engine consumes diagnoses rather than raw diffs. Rules currently cover ELF path leakage plus operational normalization for `SOURCE_DATE_EPOCH`, source mtimes, and umask. The path rule is:

```text
confirmed path intervention
 + path present in ELF debug section
 → compiler path-remapping candidate
```

Path rules may emit a narrowly scoped `ProjectPatch` for a regular root `Makefile`, `CMakeLists.txt`, or `meson.build`, or create `.cargo/config.toml` when a Cargo project has no existing Cargo config. Existing-file operations are append or insert-after-line and carry the original file SHA-256 as a stale-patch precondition; create operations refuse to overwrite an existing path. `SOURCE_DATE_EPOCH` currently supports the Makefile rule and new Cargo config rule. Verification copies the project to a temporary source tree, applies only that candidate patch, and reruns:

```text
patched baseline × 2
original failing intervention against patched project × 2
```

Runner-level normalization is deliberately disabled for project-patch verification so it cannot mask an ineffective patch. If a candidate has no conservative project patch, verification may still use the supported operational normalization path. Only byte-stable equality across baseline and the original intervention is labeled `VERIFIED`. GNU-tar mtime/mode candidates can now patch conventional Make projects, one unambiguous CMake/Meson tar site, or one detected packaging shell-script site. No project file in the user's checkout is modified.
Existing compiler-flag values stay in memory during verification; persisted fix plans contain only ReproBisect-generated additions.

## Evidence persistence

```text
.reprobisect/runs/<experiment-id>/run-....json
.reprobisect/experiments/<experiment-id>.json
.reprobisect/fixes/<diagnosis-experiment-id>.json
```

Evidence files are create-only. Ordinary `[build.env]` values are redacted in run manifests; explicit controlled values remain visible because they are part of the experiment.

## Phase 9 bounded dependency provenance

Runtime dependency-cache provenance is now explicitly bounded per cache root by `dependency_cache_max_files` and `dependency_cache_max_bytes`. The in-container summarizer stops hashing once either budget is exhausted and records the snapshot as truncated. A truncated before/after pair is never promoted to a confirmed cache mutation; at most it is retained as an incomplete observed difference. The bounds constrain content hashing cost, not directory enumeration cost, which remains best-effort because the package-manager cache lives inside the build container.

When both runtime dependency provenance and network tracing are enabled, ReproBisect derives a redacted **build-window co-occurrence** record. It uses only successful non-local endpoint-scope counts and complete cache mutations. This is intentionally weaker than byte provenance: the model cannot associate a specific network response with a specific cache object.

Parsed dependency-resolution summaries now cover Cargo, npm, pnpm, yarn, Poetry, Pipenv, uv, pinned requirements, Go, Bundler, Composer, and Gradle. Raw coordinates are used only transiently to compute a normalized SHA-256; persisted evidence keeps ecosystem, count, and digest.

For archive metadata fixes, Make-driven GNU tar projects can receive a temporary project patch using `TAR_OPTIONS`. Source-mtime diagnoses use `--mtime=@... --clamp-mtime`; umask/mode diagnoses use `--mode=u+rwX,go+rX,go-w`. Verification applies the patch only to a temporary source copy and replays the original mtime/umask intervention. Unsupported build systems still fall back to operational normalization.

## Phase 10 process-aware syscall provenance

Phase 10 adds an opt-in syscall provenance layer above the Phase 9 build-window summaries. `network_trace` and `file_input_trace` are independent collection modes. If either is enabled, the Docker runner grants `SYS_PTRACE` only to that build container, performs a matching `strace` preflight, and falls back to an untraced execution if tracing is unavailable. Instrumentation failure therefore cannot replace the build command's own exit status.

A single ephemeral syscall trace is collected under `/reprobisect-meta` and reduced into two redacted views. The network view retains syscall counts, success/failure counts, address-family/scope classes, and successful non-local events by coarse process role. The file/process view retains anonymous process keys (`p1`, `p2`, ...), coarse role, successful dependency/cache reads, declared-output writes, and same-process co-occurrence counters. Raw PID values, paths, argv, endpoints, and syscall text are discarded with the metadata directory. Configured dependency-cache roots are also `skip_serializing` runtime state.

`syscall_trace_max_bytes` bounds how many trace bytes are parsed (32 MiB by default; 1-512 MiB accepted). A truncated trace is marked incomplete. This is a parser/memory bound, not a bound on temporary trace-file disk growth.

Same-process co-occurrence is stronger than build-window correlation but remains observational provenance rather than dataflow proof. Phase 10 deliberately stops at process-local open/write evidence.

Project-level archive fixes also gain a single-site `ReplaceText` operation with the same stale-file SHA-256 precondition as other existing-file patches. CMake, Meson, and configured shell packaging scripts are patched only when exactly one recognized literal tar site exists; ambiguous multi-site logic is deliberately refused. Verification still applies the candidate exclusively in a temporary project copy and replays the original causal intervention.

## Phase 11 lineage-aware syscall provenance

Phase 11 strengthens the same redacted trace reducer without changing the causal standard. The parser keeps descriptor identity and raw paths only in memory while reducing the bounded trace. Dependency/cache descriptors are followed across `dup`, `dup2`, and `dup3`, inherited through traced `fork`/`vfork`/`clone` edges, and recognized when consumed by a readable `mmap`. A successful `rename`/`renameat`/`renameat2` whose destination is a declared output is recorded as output publication. If the source path was previously opened for writing, the reducer can additionally distinguish publication by the same anonymous process from publication across one traced lineage. Temporary path strings never enter the persisted summary.

The persisted process graph contains only anonymous IDs, anonymous parent IDs, lineage depth, coarse roles, and counts. It adds strict-ancestor co-occurrence counters, so an input/cache/network observation in an ancestor can be related to output activity in a descendant without asserting that bytes flowed between them. Active processes plus the minimum traced ancestors needed to explain those links are retained; raw PIDs are discarded.

The claim boundary remains conservative. Descriptor/path/lineage correlation is **observational provenance**, not taint analysis. ReproBisect still does not reconstruct IPC or shared-memory dataflow, `close_range`, platform-specific mapping variants, unresolved relative-path semantics under arbitrary `dirfd`/cwd changes, or transformations performed wholly in memory. A truncated trace can never support a completeness claim.


## Phase 12 artifact and package-format breadth

Phase 12 keeps byte-for-byte SHA-256 as the equality contract but broadens the type system used to explain differences. The generic collector now recognizes ELF, PE/COFF, Mach-O (including fat/universal magics), WebAssembly, ZIP, gzip, ar, and TAR without shelling out to host analyzers. PE detection validates the DOS `e_lfanew` pointer against a bounded seek before accepting the `PE\0\0` signature; TAR detection recognizes `ustar` structurally instead of relying only on the filename extension.

Archive metadata is then used to refine container formats. ZIP structures can be classified as JAR or Python wheel, ar structures as Debian packages, and TAR structures as OCI image layouts. Extension hints (`.jar`, `.whl`, `.deb`) are accepted only after the corresponding container magic is present; extensionless artifacts can still be refined from standard member structure. The existing bounded archive parser and metadata comparison engine is reused, so member count/order/mtime/uid/gid/mode/size evidence now applies directly to these ecosystem-specific artifacts without adding external parser dependencies.

The Phase 12 refinement is intentionally structural. Phase 17 adds a second bounded semantic layer without executing artifact content or invoking host-specific package tools. For ZIP-derived formats it reads only small unencrypted `ZIP_STORED` metadata members; compressed members remain structural-only until an explicitly bounded decompressor is introduced. DEB analysis reads the outer ar control/data member identities and `debian-binary`; OCI analysis reads only bounded `oci-layout` and `index.json`; executable formats use bounded headers only.

## Phase 17 bounded semantic artifact evidence

Each `ArtifactRecord` may now contain an `ArtifactSemanticSummary` consisting of a format kind, a bounded string-valued attribute map, and a truncation bit. Selected examples include JAR manifest fields/class counts, wheel WHEEL/METADATA/RECORD fingerprints, DEB control/data archive identities, OCI layout/index schema and manifest-digest-set fingerprints, PE/COFF machine/section/timestamp fields, Mach-O header/UUID information, and Wasm section/custom-section structure.

Semantic summaries are compared only after artifact bytes change. The resulting field-level deltas are persisted on `ArtifactDelta`. They remain supporting evidence: identifying that a wheel's `metadata_version` changed does not prove why it changed. One narrow causal promotion is currently defined: a PE/COFF `coff_timestamp` delta can count as direct structural evidence when produced by the controlled `SOURCE_DATE_EPOCH` intervention. The corresponding remediation is toolchain-agnostic and recommends the selected PE linker/toolchain's deterministic timestamp/reproducible-build mode rather than guessing a vendor-specific flag.

Analysis is bounded to 64 MiB per semantic artifact with embedded metadata members capped at 256 KiB and Wasm section traversal capped at 4096 sections. Unsupported/compressed metadata is omitted rather than decompressed without a budget.

## OCI runtime selection

`[build].runner` is a typed `docker | podman` choice, defaulting to Docker for backward compatibility. `OciRunner` uses the selected executable consistently for runtime availability, image inspect/pull, toolchain probes, build containers, timeout kills, and verified-fix replays. The runtime choice is persisted as `runner_backend` in build evidence.

Runtime selection is deliberately configuration, not an intervention dimension: ReproBisect does not claim that a Docker-vs-Podman delta isolates a build cause because the two runtimes may differ in filesystem, namespace, networking, security, and image-storage behavior simultaneously.

## Known-environment delta minimization

`reprobisect compare` is a separate causal workflow for a different input condition: the user supplies a known-good and known-bad controlled-environment manifest. Both are applied on top of the canonical baseline, so tracing/provenance instrumentation and build specification remain identical unless they are themselves explicit supported comparison dimensions.

The workflow is:

```text
repeat known-good -> require stable successful artifact signature G
repeat known-bad  -> require either:
                     (a) stable artifact signature B != G, or
                     (b) stable build-failure signature F
compute supported controlled delta D
        ↓
ddmin subsets of D using real build attempts
        ↓
artifact predicate = repeated runs exactly equal B
failure predicate  = repeated failures exactly equal F
        ↓
rerun minimal subset
        ↓
optionally rerun G as temporal reversion confirmation
```

A failure signature is the tuple `(exit_code, sha256(stdout), sha256(stderr))`. Matching exit code alone is deliberately insufficient because unrelated build failures commonly reuse generic process exit codes. Mixed success/failure endpoints or changing failure diagnostics are treated as unstable and minimization is refused.

These exact-bad predicates prevent a subset that merely creates a third artifact or an unrelated non-zero process result from being mistaken for an explanation of the known-bad outcome. The returned set is 1-minimal under ddmin's tested path, not globally minimal under arbitrary unmodeled environments.

Environment manifests cannot select the container runtime; runner identity remains fixed by `[build].runner` for the entire comparison. They also cannot change tracing/provenance collection knobs, which keeps instrumentation from entering the candidate delta.

## Phase 19 persisted-evidence compatibility boundary

Schema numbers are centralized in `src/schema.rs`. Evidence producers use those constants directly; compatibility code owns the oldest supported version for each persisted document family. `src/compat.rs` performs structural kind detection, validates the top-level version before deserialization, recursively migrates nested build evidence, and only then deserializes into the current model.

This ordering is intentional. Many model additions use `#[serde(default)]` for forward evolution inside the codebase. Calling `serde_json::from_value` before checking the declared schema could therefore make an unsupported old or future document appear valid simply because enough fields defaulted. The compatibility layer treats `schema_version` as an admission check rather than advisory metadata.

The reader is bounded to 64 MiB and rejects unsupported future versions. It never modifies source evidence implicitly. Historical full-log migration preserves the old persisted-text identity rather than claiming to reconstruct raw bytes that older releases did not retain; historical unbounded cache summaries similarly carry explicit unknown-bound sentinels instead of invented limits.
