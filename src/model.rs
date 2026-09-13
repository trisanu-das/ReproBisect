use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;


#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RunnerBackend {
    #[default]
    Docker,
    Podman,
}

impl RunnerBackend {
    pub const fn executable(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Podman => "podman",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Docker => "Docker",
            Self::Podman => "Podman",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildSpec {
    pub image: String,
    pub command: Vec<String>,
    pub outputs: Vec<PathBuf>,
    pub environment: BTreeMap<String, String>,
    pub working_directory: PathBuf,
    pub timeout_seconds: u64,
    pub log_capture_max_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlledEnvironment {
    /// Optional build-image override used for toolchain/container interventions.
    #[serde(default)]
    pub image_override: Option<String>,
    pub container_source_path: String,
    pub container_work_path: String,
    #[serde(default)]
    pub source_copy_order: SourceCopyOrder,
    pub environment: BTreeMap<String, String>,
    /// Explicit compiler/linker/archive-tool bindings. These are separated from
    /// ordinary environment variables so the runner can probe the executable
    /// that was actually selected inside the build container.
    #[serde(default)]
    pub toolchain_bindings: BTreeMap<String, String>,
    /// Target -> variant source file substitutions applied only in the fresh
    /// workspace. Paths are project-relative and never mutate the source tree.
    #[serde(default)]
    pub source_file_overrides: BTreeMap<PathBuf, PathBuf>,
    pub hostname: Option<String>,
    pub cpu_count: Option<u32>,
    #[serde(default)]
    pub source_mtime_epoch: Option<i64>,
    #[serde(default)]
    pub umask: Option<u32>,
    pub network_mode: String,
    /// Best-effort syscall tracing for network provenance. Raw traces are never
    /// persisted; only a redacted summary is retained in BuildRun evidence.
    #[serde(default)]
    pub network_trace: bool,
    /// Best-effort process-aware file-input/output provenance.
    #[serde(default)]
    pub file_input_trace: bool,
    /// Maximum raw trace bytes parsed after a run.
    #[serde(default = "default_syscall_trace_max_bytes")]
    pub syscall_trace_max_bytes: u64,
    /// Collect post-build dependency resolution/cache provenance. Only aggregate
    /// hashes/counts are persisted; cache paths and package coordinates are not.
    #[serde(default)]
    pub runtime_dependency_provenance: bool,
    /// Optional package-manager cache roots inside the build container, keyed by
    /// ecosystem label. Values are never persisted in run evidence.
    #[serde(default, skip_serializing)]
    pub dependency_cache_paths: BTreeMap<String, String>,
    /// Maximum number of cache files whose contents may be hashed per cache root.
    /// Directory traversal remains best-effort, but content hashing is bounded.
    #[serde(default = "default_dependency_cache_max_files")]
    pub dependency_cache_max_files: usize,
    /// Maximum cumulative bytes hashed per cache root.
    #[serde(default = "default_dependency_cache_max_bytes")]
    pub dependency_cache_max_bytes: u64,
}

impl Default for ControlledEnvironment {
    fn default() -> Self {
        Self {
            image_override: None,
            container_source_path: "/src".to_string(),
            container_work_path: "/workspace".to_string(),
            source_copy_order: SourceCopyOrder::Sorted,
            environment: BTreeMap::new(),
            toolchain_bindings: BTreeMap::new(),
            source_file_overrides: BTreeMap::new(),
            hostname: None,
            cpu_count: None,
            source_mtime_epoch: None,
            umask: None,
            network_mode: "default".to_string(),
            network_trace: false,
            file_input_trace: false,
            syscall_trace_max_bytes: default_syscall_trace_max_bytes(),
            runtime_dependency_provenance: false,
            dependency_cache_paths: BTreeMap::new(),
            dependency_cache_max_files: default_dependency_cache_max_files(),
            dependency_cache_max_bytes: default_dependency_cache_max_bytes(),
        }
    }
}


fn default_dependency_cache_max_files() -> usize { 2048 }
fn default_dependency_cache_max_bytes() -> u64 { 128 * 1024 * 1024 }
fn default_syscall_trace_max_bytes() -> u64 { 32 * 1024 * 1024 }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceCopyOrder {
    Sorted,
    Reverse,
}

impl Default for SourceCopyOrder {
    fn default() -> Self {
        Self::Sorted
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmbeddedMarker {
    pub variable: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ElfMarkerLocation {
    pub variable: String,
    pub value: String,
    pub section: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ElfMetadataSummary {
    pub build_id: Option<String>,
    pub debug_sections: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveMemberMetadata {
    pub index: usize,
    pub name: String,
    pub size: u64,
    pub mtime: Option<i64>,
    pub uid: Option<u64>,
    pub gid: Option<u64>,
    pub mode: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveMetadataSummary {
    pub format: String,
    pub member_count: usize,
    pub members: Vec<ArchiveMemberMetadata>,
    pub truncated: bool,
    pub container_mtime: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactSemanticSummary {
    pub kind: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactSemanticDifference {
    pub field: String,
    pub baseline: Option<String>,
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub logical_path: PathBuf,
    pub sha256: String,
    pub size_bytes: u64,
    pub detected_type: ArtifactType,
    #[serde(default)]
    pub embedded_markers: Vec<EmbeddedMarker>,
    #[serde(default)]
    pub elf_debug_markers: Vec<EmbeddedMarker>,
    #[serde(default)]
    pub elf_marker_locations: Vec<ElfMarkerLocation>,
    #[serde(default)]
    pub elf_metadata: Option<ElfMetadataSummary>,
    #[serde(default)]
    pub archive_metadata: Option<ArchiveMetadataSummary>,
    #[serde(default)]
    pub semantic_metadata: Option<ArtifactSemanticSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    Elf,
    PeCoff,
    MachO,
    Wasm,
    Zip,
    Jar,
    PythonWheel,
    Tar,
    OciImage,
    Gzip,
    Ar,
    Deb,
    Generic,
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolchainProbe {
    pub tool: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolchainBindingProbe {
    pub variable: String,
    pub configured_value: String,
    #[serde(default)]
    pub resolved_path: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    /// Number of build-time invocations observed through ReproBisect's stable
    /// wrapper. Arguments are intentionally not recorded.
    #[serde(default)]
    pub invocation_count: usize,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolchainProvenance {
    #[serde(default)]
    pub probes: Vec<ToolchainProbe>,
    #[serde(default)]
    pub bindings: Vec<ToolchainBindingProbe>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceOverrideRecord {
    pub target: PathBuf,
    pub variant_source: PathBuf,
    pub baseline_sha256: String,
    pub variant_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct NetworkTraceSummary {
    pub attempted: bool,
    pub tracer_available: bool,
    #[serde(default)]
    pub socket_calls: usize,
    pub connect_calls: usize,
    #[serde(default)]
    pub successful_connects: usize,
    #[serde(default)]
    pub failed_connects: usize,
    pub sendto_calls: usize,
    #[serde(default)]
    pub recvfrom_calls: usize,
    #[serde(default)]
    pub address_families: Vec<String>,
    /// Redacted endpoint classes such as public/private/loopback/unix. Exact
    /// addresses are never persisted.
    #[serde(default)]
    pub endpoint_scopes: BTreeMap<String, usize>,
    /// Endpoint scopes for network syscalls that completed successfully. This is
    /// still coarse provenance: no exact address, port, hostname, or payload is retained.
    #[serde(default)]
    pub successful_endpoint_scopes: BTreeMap<String, usize>,
    /// Successful non-local network events grouped only by coarse process role.
    #[serde(default)]
    pub successful_nonlocal_by_role: BTreeMap<String, usize>,
    /// True when syscall parsing stopped at the configured byte budget.
    #[serde(default)]
    pub trace_truncated: bool,
    #[serde(default)]
    pub parsed_bytes: u64,
    #[serde(default)]
    pub error: Option<String>,
}



#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProcessActivitySummary {
    /// Anonymous stable-within-run process key (for example p1).
    pub process: String,
    /// Anonymous parent process when the traced process was created by a traced
    /// fork/clone/vfork edge. Raw container PIDs are never persisted.
    #[serde(default)]
    pub parent_process: Option<String>,
    /// Number of traced parent edges between this process and the first traced
    /// ancestor retained in the provenance summary.
    #[serde(default)]
    pub lineage_depth: usize,
    /// Coarse executable role inferred from an ephemeral execve path.
    pub role: String,
    #[serde(default)]
    pub dependency_reads: usize,
    /// Successful readable mmap operations backed by a dependency descriptor.
    #[serde(default)]
    pub dependency_mmaps: usize,
    #[serde(default)]
    pub cache_reads: usize,
    /// Successful readable mmap operations backed by a dependency-cache descriptor.
    #[serde(default)]
    pub cache_mmaps: usize,
    #[serde(default)]
    pub output_writes: usize,
    /// Successful rename/renameat/renameat2 operations publishing a declared output.
    #[serde(default)]
    pub output_publications: usize,
    /// Output publications whose source path was previously opened for writing
    /// by the same traced process. Paths remain ephemeral and are not serialized.
    #[serde(default)]
    pub temp_output_publications: usize,
    /// Output publications whose source path was previously opened for writing
    /// by a traced ancestor or descendant process.
    #[serde(default)]
    pub lineage_temp_output_publications: usize,
    #[serde(default)]
    pub successful_nonlocal_network_events: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProcessTraceSummary {
    pub attempted: bool,
    pub tracer_available: bool,
    /// True when only the first configured byte budget of the raw trace was parsed.
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub parsed_bytes: u64,
    #[serde(default)]
    pub processes: Vec<ProcessActivitySummary>,
    #[serde(default)]
    pub same_process_dependency_output: usize,
    #[serde(default)]
    pub same_process_cache_output: usize,
    #[serde(default)]
    pub same_process_network_cache: usize,
    #[serde(default)]
    pub same_process_network_cache_output: usize,
    /// Output-producing descendants whose strict ancestor chain contains a
    /// dependency read/open or readable dependency-backed mmap observation.
    #[serde(default)]
    pub ancestor_dependency_output: usize,
    /// Output-producing descendants whose strict ancestor chain contains a
    /// dependency-cache read/open or readable cache-backed mmap observation.
    #[serde(default)]
    pub ancestor_cache_output: usize,
    /// Output-producing descendants whose strict ancestor chain contains a
    /// successful known non-local network event.
    #[serde(default)]
    pub ancestor_network_output: usize,
    /// Output-producing descendants whose strict ancestor chain cumulatively
    /// contains both successful known non-local networking and cache activity.
    #[serde(default)]
    pub ancestor_network_cache_output: usize,
    #[serde(default)]
    pub dependency_mmaps: usize,
    #[serde(default)]
    pub cache_mmaps: usize,
    #[serde(default)]
    pub output_publications: usize,
    #[serde(default)]
    pub temp_output_publications: usize,
    #[serde(default)]
    pub lineage_temp_output_publications: usize,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyCacheSummary {
    pub ecosystem: String,
    pub before_file_count: usize,
    pub before_total_bytes: u64,
    #[serde(default)]
    pub before_aggregate_sha256: Option<String>,
    #[serde(default)]
    pub before_truncated: bool,
    pub after_file_count: usize,
    pub after_total_bytes: u64,
    #[serde(default)]
    pub after_aggregate_sha256: Option<String>,
    #[serde(default)]
    pub after_truncated: bool,
    /// The configured hashing bounds that produced this summary. A value of 0 is
    /// reserved by the Phase 19 compatibility migrator for pre-Phase9 evidence,
    /// where the historical run did not persist a bound.
    pub max_files: usize,
    pub max_bytes: u64,
    /// True only when both snapshots were complete and differed.
    pub changed_during_build: bool,
    /// True when the bounded summaries differ even if one side was truncated.
    /// This is intentionally weaker than `changed_during_build`.
    pub observed_change: bool,
    /// Whether both before/after cache snapshots fit within the configured bounds.
    pub comparison_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DependencyNetworkCorrelation {
    pub attempted: bool,
    pub network_trace_available: bool,
    pub successful_nonlocal_network_events: usize,
    pub complete_cache_mutations: usize,
    pub incomplete_cache_observations: usize,
    /// Co-occurrence within the build window only. This is not byte-level provenance.
    pub build_window_cooccurrence: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeDependencyProvenance {
    pub attempted: bool,
    /// Dependency declaration/lockfile state after the build has completed.
    #[serde(default)]
    pub dependency_files: Vec<DependencyFileRecord>,
    #[serde(default)]
    pub dependency_resolutions: Vec<DependencyResolutionRecord>,
    /// Before/after aggregate content fingerprints for known/configured package-manager
    /// cache roots. File names and cache paths are not retained.
    #[serde(default)]
    pub cache_summaries: Vec<DependencyCacheSummary>,
    #[serde(default)]
    pub network_cache_correlation: DependencyNetworkCorrelation,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyFileRecord {
    pub path: PathBuf,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyResolutionRecord {
    pub path: PathBuf,
    pub ecosystem: String,
    pub parsed: bool,
    pub package_count: usize,
    #[serde(default)]
    pub normalized_sha256: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SourceProvenance {
    #[serde(default)]
    pub git_commit: Option<String>,
    #[serde(default)]
    pub git_dirty: Option<bool>,
    #[serde(default)]
    pub dependency_files: Vec<DependencyFileRecord>,
    #[serde(default)]
    pub dependency_resolutions: Vec<DependencyResolutionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildRun {
    pub schema_version: u32,
    pub experiment_id: Uuid,
    pub run_id: Uuid,
    pub ordinal: usize,
    #[serde(default)]
    pub runner_backend: RunnerBackend,
    pub source_digest: String,
    pub image: String,
    pub resolved_image_id: String,
    pub command: Vec<String>,
    pub working_directory: PathBuf,
    pub effective_environment: BTreeMap<String, String>,
    pub controlled_environment: ControlledEnvironment,
    #[serde(default)]
    pub toolchain_provenance: ToolchainProvenance,
    #[serde(default)]
    pub source_overrides: Vec<SourceOverrideRecord>,
    #[serde(default)]
    pub network_trace: NetworkTraceSummary,
    #[serde(default)]
    pub process_trace: ProcessTraceSummary,
    #[serde(default)]
    pub runtime_dependency_provenance: RuntimeDependencyProvenance,
    pub exit_code: i32,
    pub duration_ms: u128,
    #[serde(default)]
    pub log_capture_max_bytes: u64,
    pub stdout: String,
    pub stderr: String,
    #[serde(default)]
    pub stdout_sha256: String,
    #[serde(default)]
    pub stderr_sha256: String,
    #[serde(default)]
    pub stdout_bytes: u64,
    #[serde(default)]
    pub stderr_bytes: u64,
    #[serde(default)]
    pub stdout_truncated: bool,
    #[serde(default)]
    pub stderr_truncated: bool,
    pub artifacts: Vec<ArtifactRecord>,
}

/// A build process that started successfully but exited non-zero. Infrastructure
/// failures (configured OCI runtime unavailable, timeout, invalid mounts, etc.) remain `Err` so
/// they cannot be mistaken for evidence about the build itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildFailure {
    pub schema_version: u32,
    pub experiment_id: Uuid,
    pub run_id: Uuid,
    pub ordinal: usize,
    #[serde(default)]
    pub runner_backend: RunnerBackend,
    pub source_digest: String,
    pub image: String,
    pub resolved_image_id: String,
    pub command: Vec<String>,
    pub working_directory: PathBuf,
    pub effective_environment: BTreeMap<String, String>,
    pub controlled_environment: ControlledEnvironment,
    #[serde(default)]
    pub toolchain_provenance: ToolchainProvenance,
    #[serde(default)]
    pub source_overrides: Vec<SourceOverrideRecord>,
    #[serde(default)]
    pub network_trace: NetworkTraceSummary,
    #[serde(default)]
    pub process_trace: ProcessTraceSummary,
    #[serde(default)]
    pub runtime_dependency_provenance: RuntimeDependencyProvenance,
    pub exit_code: i32,
    pub duration_ms: u128,
    #[serde(default)]
    pub log_capture_max_bytes: u64,
    pub stdout: String,
    pub stderr: String,
    #[serde(default)]
    pub stdout_sha256: String,
    #[serde(default)]
    pub stderr_sha256: String,
    #[serde(default)]
    pub stdout_bytes: u64,
    #[serde(default)]
    pub stderr_bytes: u64,
    #[serde(default)]
    pub stdout_truncated: bool,
    #[serde(default)]
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Reproducible,
    NonReproducible,
    UncontrolledNondeterminism,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactComparison {
    pub logical_path: PathBuf,
    pub equal_across_runs: bool,
    pub hashes: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InterventionKind {
    BuildImage,
    ToolchainExecutable,
    DependencyFile,
    NetworkAccess,
    SourcePath,
    BuildPath,
    SourceDateEpoch,
    SourceMtime,
    Timezone,
    Locale,
    Hostname,
    EnvironmentVariable,
    CpuCount,
    Umask,
    DirectoryOrder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intervention {
    pub id: String,
    pub kind: InterventionKind,
    pub variable: String,
    pub baseline_value: String,
    pub variant_value: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveMetadataField {
    MemberCount,
    MemberOrder,
    Mtime,
    Uid,
    Gid,
    Mode,
    Size,
    ContainerMtime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveMetadataDifference {
    pub field: ArchiveMetadataField,
    pub member: Option<String>,
    pub baseline: Option<String>,
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactDelta {
    pub logical_path: PathBuf,
    pub baseline_sha256: String,
    pub variant_sha256: String,
    pub changed: bool,
    #[serde(default)]
    pub marker_evidence: Vec<EmbeddedMarker>,
    #[serde(default)]
    pub elf_debug_marker_evidence: Vec<EmbeddedMarker>,
    #[serde(default)]
    pub elf_marker_location_evidence: Vec<ElfMarkerLocation>,
    #[serde(default)]
    pub archive_metadata_evidence: Vec<ArchiveMetadataDifference>,
    #[serde(default)]
    pub semantic_evidence: Vec<ArtifactSemanticDifference>,
    #[serde(default)]
    pub baseline_direct_marker: bool,
    #[serde(default)]
    pub variant_direct_marker: bool,
    #[serde(default)]
    pub direct_structural_evidence: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StochasticEffectClassification {
    Supported,
    NotSupported,
    BaselineUnstable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StochasticEffectEvidence {
    pub baseline_trials: usize,
    pub variant_trials: usize,
    pub baseline_changed_trials: usize,
    pub variant_changed_trials: usize,
    pub baseline_change_rate: f64,
    pub variant_change_rate: f64,
    pub absolute_rate_difference: f64,
    pub fisher_exact_p_value: f64,
    pub alpha: f64,
    pub classification: StochasticEffectClassification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterventionResult {
    pub intervention: Intervention,
    pub runs: Vec<BuildRun>,
    /// Matched baseline trials used by stochastic interventions. Empty for
    /// deterministic interventions.
    #[serde(default)]
    pub reference_runs: Vec<BuildRun>,
    #[serde(default)]
    pub build_failures: Vec<BuildFailure>,
    pub artifact_deltas: Vec<ArtifactDelta>,
    pub changed: bool,
    #[serde(default)]
    pub variant_stable: Option<bool>,
    #[serde(default)]
    pub distinct_variant_outcomes: usize,
    #[serde(default)]
    pub stochastic_effect: Option<StochasticEffectEvidence>,
    pub reverted_to_baseline: Option<bool>,
    pub confirmation_run: Option<BuildRun>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionSearchResult {
    pub candidate_variables: Vec<String>,
    pub minimal_variables: Vec<String>,
    pub tested_subsets: usize,
    pub runs: Vec<BuildRun>,
    pub artifact_deltas: Vec<ArtifactDelta>,
    pub changed: bool,
    pub stable_effect: bool,
    pub reverted_to_baseline: Option<bool>,
    pub confirmation_run: Option<BuildRun>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvironmentDeltaRecord {
    pub variable: String,
    pub baseline_value: String,
    pub variant_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentComparisonStatus {
    Equivalent,
    Minimized,
    Inconclusive,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentComparisonOutcomeKind {
    #[default]
    ArtifactSuccess,
    BuildFailure,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildFailureSignature {
    pub exit_code: i32,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentComparisonReport {
    pub schema_version: u32,
    pub experiment_id: Uuid,
    pub status: EnvironmentComparisonStatus,
    pub source_digest: String,
    #[serde(default)]
    pub source_provenance: SourceProvenance,
    pub good_manifest_sha256: String,
    pub bad_manifest_sha256: String,
    #[serde(default)]
    pub good_outcome_kind: EnvironmentComparisonOutcomeKind,
    #[serde(default)]
    pub bad_outcome_kind: EnvironmentComparisonOutcomeKind,
    pub good_runs: Vec<BuildRun>,
    #[serde(default)]
    pub good_failures: Vec<BuildFailure>,
    pub bad_runs: Vec<BuildRun>,
    #[serde(default)]
    pub bad_failures: Vec<BuildFailure>,
    #[serde(default)]
    pub bad_failure_signature: Option<BuildFailureSignature>,
    #[serde(default)]
    pub artifact_deltas: Vec<ArtifactDelta>,
    #[serde(default)]
    pub delta: Vec<EnvironmentDeltaRecord>,
    #[serde(default)]
    pub minimal_delta: Vec<EnvironmentDeltaRecord>,
    pub tested_subsets: usize,
    #[serde(default)]
    pub reproduction_runs: Vec<BuildRun>,
    #[serde(default)]
    pub reproduction_failures: Vec<BuildFailure>,
    pub reverted_to_good: Option<bool>,
    pub confirmation_run: Option<BuildRun>,
    #[serde(default)]
    pub confirmation_failure: Option<BuildFailure>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnosis {
    pub title: String,
    pub causal_variables: Vec<String>,
    pub affected_artifacts: Vec<PathBuf>,
    pub evidence: Vec<String>,
    pub confidence: Confidence,
    pub remediation: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckReport {
    pub schema_version: u32,
    pub experiment_id: Uuid,
    pub status: CheckStatus,
    pub source_digest: String,
    #[serde(default)]
    pub source_provenance: SourceProvenance,
    pub baseline_environment: ControlledEnvironment,
    pub runs: Vec<BuildRun>,
    pub artifact_comparisons: Vec<ArtifactComparison>,
    #[serde(default)]
    pub interventions: Vec<InterventionResult>,
    #[serde(default)]
    pub interaction_search: Option<InteractionSearchResult>,
    #[serde(default)]
    pub diagnoses: Vec<Diagnosis>,
    pub notes: Vec<String>,
}
