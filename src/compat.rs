use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    engine::FixReport,
    model::{BuildFailure, BuildRun, CheckReport, EnvironmentComparisonReport},
    schema::{
        BUILD_EVIDENCE_SCHEMA_CURRENT, EvidenceKind, CHECK_REPORT_SCHEMA_CURRENT,
        ENVIRONMENT_COMPARISON_SCHEMA_CURRENT, FIX_REPORT_SCHEMA_CURRENT,
    },
};

const MAX_EVIDENCE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceCompatibilitySummary {
    pub kind: EvidenceKind,
    pub source_schema_version: u32,
    pub normalized_schema_version: u32,
    pub migrated: bool,
    pub build_documents_migrated: usize,
    pub derived_legacy_log_digests: usize,
    pub legacy_cache_summaries_migrated: usize,
    pub notes: Vec<String>,
}

#[derive(Debug)]
pub enum EvidenceDocument {
    CheckReport(CheckReport),
    BuildRun(BuildRun),
    BuildFailure(BuildFailure),
    FixReport(FixReport),
    EnvironmentComparisonReport(EnvironmentComparisonReport),
}

impl EvidenceDocument {
    pub fn kind(&self) -> EvidenceKind {
        match self {
            Self::CheckReport(_) => EvidenceKind::CheckReport,
            Self::BuildRun(_) => EvidenceKind::BuildRun,
            Self::BuildFailure(_) => EvidenceKind::BuildFailure,
            Self::FixReport(_) => EvidenceKind::FixReport,
            Self::EnvironmentComparisonReport(_) => EvidenceKind::EnvironmentComparisonReport,
        }
    }

    pub fn to_value(&self) -> Result<Value> {
        match self {
            Self::CheckReport(value) => serde_json::to_value(value),
            Self::BuildRun(value) => serde_json::to_value(value),
            Self::BuildFailure(value) => serde_json::to_value(value),
            Self::FixReport(value) => serde_json::to_value(value),
            Self::EnvironmentComparisonReport(value) => serde_json::to_value(value),
        }
        .context("cannot serialize normalized evidence")
    }
}

#[derive(Debug)]
pub struct LoadedEvidence {
    pub source_path: PathBuf,
    pub summary: EvidenceCompatibilitySummary,
    pub document: EvidenceDocument,
}

#[derive(Debug, Default)]
struct MigrationStats {
    build_documents_migrated: usize,
    derived_legacy_log_digests: usize,
    legacy_cache_summaries_migrated: usize,
    notes: Vec<String>,
}

pub fn load_evidence(path: &Path) -> Result<LoadedEvidence> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("cannot stat evidence file {}", path.display()))?;
    if !metadata.is_file() {
        bail!("evidence path is not a regular file: {}", path.display());
    }
    if metadata.len() > MAX_EVIDENCE_BYTES {
        bail!(
            "evidence file {} is {} bytes; maximum supported input is {} bytes",
            path.display(),
            metadata.len(),
            MAX_EVIDENCE_BYTES
        );
    }

    let mut file = fs::File::open(path)
        .with_context(|| format!("cannot open evidence file {}", path.display()))?;
    let mut raw = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_EVIDENCE_BYTES.saturating_add(1))
        .read_to_end(&mut raw)
        .with_context(|| format!("cannot read evidence file {}", path.display()))?;
    if raw.len() as u64 > MAX_EVIDENCE_BYTES {
        bail!(
            "evidence file {} exceeded the {} byte input limit while reading",
            path.display(),
            MAX_EVIDENCE_BYTES
        );
    }
    let mut value: Value = serde_json::from_slice(&raw)
        .with_context(|| format!("cannot parse evidence JSON {}", path.display()))?;
    let kind = detect_kind(&value)?;
    let source_schema_version = schema_version(&value, kind)?;
    validate_schema_version(kind, source_schema_version)?;

    let mut stats = MigrationStats::default();
    match kind {
        EvidenceKind::CheckReport => migrate_check_report(&mut value, &mut stats)?,
        EvidenceKind::BuildRun => migrate_build_run(&mut value, &mut stats)?,
        EvidenceKind::BuildFailure => migrate_build_failure(&mut value, &mut stats)?,
        EvidenceKind::FixReport => migrate_fix_report(&mut value, &mut stats)?,
        EvidenceKind::EnvironmentComparisonReport => {
            migrate_environment_comparison(&mut value, &mut stats)?
        }
    }

    let document = deserialize_current(kind, value)?;
    let normalized_schema_version = kind.current_schema_version();
    let migrated = source_schema_version != normalized_schema_version
        || stats.build_documents_migrated > 0
        || stats.derived_legacy_log_digests > 0
        || stats.legacy_cache_summaries_migrated > 0;

    if source_schema_version != normalized_schema_version {
        stats.notes.push(format!(
            "normalized {} schema {} to current schema {}",
            kind.label(),
            source_schema_version,
            normalized_schema_version
        ));
    }
    if stats.derived_legacy_log_digests > 0 {
        stats.notes.push(format!(
            "derived legacy log hashes/byte counts from {} run/failure record(s) using the complete persisted UTF-8 text; this preserves pre-Phase18 failure identity but cannot recover original non-UTF8 raw stream bytes",
            stats.derived_legacy_log_digests
        ));
    }
    if stats.legacy_cache_summaries_migrated > 0 {
        stats.notes.push(format!(
            "migrated {} pre-bounded dependency-cache summary record(s); max_files/max_bytes are normalized to 0 to mean unknown historical bounds",
            stats.legacy_cache_summaries_migrated
        ));
    }

    Ok(LoadedEvidence {
        source_path: path.to_path_buf(),
        summary: EvidenceCompatibilitySummary {
            kind,
            source_schema_version,
            normalized_schema_version,
            migrated,
            build_documents_migrated: stats.build_documents_migrated,
            derived_legacy_log_digests: stats.derived_legacy_log_digests,
            legacy_cache_summaries_migrated: stats.legacy_cache_summaries_migrated,
            notes: stats.notes,
        },
        document,
    })
}

pub fn write_normalized(
    source_path: &Path,
    path: &Path,
    document: &EvidenceDocument,
    force: bool,
) -> Result<()> {
    refuse_source_overwrite(source_path, path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                bail!(
                    "normalized evidence destination is a symlink; refusing to follow it: {}",
                    path.display()
                );
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "cannot inspect normalized evidence destination {}",
                    path.display()
                )
            });
        }
    }

    let value = document.to_value()?;
    let bytes = serde_json::to_vec_pretty(&value).context("cannot encode normalized evidence")?;
    let mut options = OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("cannot create normalized evidence file {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write normalized evidence file {}", path.display()))?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn refuse_source_overwrite(source_path: &Path, output_path: &Path) -> Result<()> {
    let source = fs::canonicalize(source_path)
        .with_context(|| format!("cannot resolve source evidence {}", source_path.display()))?;

    let output = if output_path.exists() {
        fs::canonicalize(output_path)
            .with_context(|| format!("cannot resolve output path {}", output_path.display()))?
    } else {
        let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
        let parent = fs::canonicalize(parent)
            .with_context(|| format!("cannot resolve output parent {}", parent.display()))?;
        let file_name = output_path
            .file_name()
            .context("normalized evidence output must name a file")?;
        parent.join(file_name)
    };

    if source == output {
        bail!(
            "normalized evidence output must differ from the source evidence file: {}",
            source_path.display()
        );
    }
    Ok(())
}

fn detect_kind(value: &Value) -> Result<EvidenceKind> {
    let object = value
        .as_object()
        .context("evidence JSON must have a top-level object")?;

    if object.contains_key("diagnosis")
        && object.contains_key("candidates")
        && object.contains_key("verifications")
    {
        return Ok(EvidenceKind::FixReport);
    }
    if object.contains_key("good_manifest_sha256") && object.contains_key("bad_manifest_sha256") {
        return Ok(EvidenceKind::EnvironmentComparisonReport);
    }
    if object.contains_key("baseline_environment")
        && object.contains_key("artifact_comparisons")
        && object.contains_key("status")
    {
        return Ok(EvidenceKind::CheckReport);
    }
    if object.contains_key("run_id") && object.contains_key("artifacts") {
        return Ok(EvidenceKind::BuildRun);
    }
    if object.contains_key("run_id")
        && object.contains_key("exit_code")
        && !object.contains_key("artifacts")
    {
        return Ok(EvidenceKind::BuildFailure);
    }

    bail!(
        "unrecognized ReproBisect evidence document; expected a check report, run, build failure, fix report, or environment-comparison report"
    )
}

fn schema_version(value: &Value, kind: EvidenceKind) -> Result<u32> {
    value
        .as_object()
        .and_then(|object| object.get("schema_version"))
        .and_then(Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .with_context(|| format!("{} is missing an integer schema_version", kind.label()))
}

fn validate_schema_version(kind: EvidenceKind, version: u32) -> Result<()> {
    let minimum = kind.minimum_schema_version();
    let current = kind.current_schema_version();
    if version < minimum {
        bail!(
            "unsupported legacy {} schema {}; oldest supported schema is {} (Phase 6 compatibility boundary)",
            kind.label(),
            version,
            minimum
        );
    }
    if version > current {
        bail!(
            "unsupported future {} schema {}; this ReproBisect build supports through schema {}; upgrade ReproBisect rather than guessing at a future evidence shape",
            kind.label(),
            version,
            current
        );
    }
    Ok(())
}

fn deserialize_current(kind: EvidenceKind, value: Value) -> Result<EvidenceDocument> {
    match kind {
        EvidenceKind::CheckReport => serde_json::from_value::<CheckReport>(value)
            .map(EvidenceDocument::CheckReport)
            .context("legacy check report could not be normalized to the current schema"),
        EvidenceKind::BuildRun => serde_json::from_value::<BuildRun>(value)
            .map(EvidenceDocument::BuildRun)
            .context("legacy build run could not be normalized to the current schema"),
        EvidenceKind::BuildFailure => serde_json::from_value::<BuildFailure>(value)
            .map(EvidenceDocument::BuildFailure)
            .context("legacy build failure could not be normalized to the current schema"),
        EvidenceKind::FixReport => serde_json::from_value::<FixReport>(value)
            .map(EvidenceDocument::FixReport)
            .context("legacy fix report could not be normalized to the current schema"),
        EvidenceKind::EnvironmentComparisonReport => {
            serde_json::from_value::<EnvironmentComparisonReport>(value)
                .map(EvidenceDocument::EnvironmentComparisonReport)
                .context("legacy environment comparison could not be normalized to the current schema")
        }
    }
}

fn migrate_check_report(value: &mut Value, stats: &mut MigrationStats) -> Result<()> {
    let version = schema_version(value, EvidenceKind::CheckReport)?;
    validate_schema_version(EvidenceKind::CheckReport, version)?;
    let object = object_mut(value, "check report")?;

    migrate_run_array(object.get_mut("runs"), stats, "check report runs")?;
    if let Some(interventions) = object.get_mut("interventions").and_then(Value::as_array_mut) {
        for intervention in interventions {
            let intervention = object_mut(intervention, "intervention result")?;
            migrate_run_array(intervention.get_mut("runs"), stats, "intervention runs")?;
            migrate_run_array(
                intervention.get_mut("reference_runs"),
                stats,
                "stochastic reference runs",
            )?;
            migrate_failure_array(
                intervention.get_mut("build_failures"),
                stats,
                "intervention build failures",
            )?;
            migrate_optional_run(intervention.get_mut("confirmation_run"), stats)?;
        }
    }
    if let Some(search) = object.get_mut("interaction_search") {
        if !search.is_null() {
            let search = object_mut(search, "interaction search")?;
            migrate_run_array(search.get_mut("runs"), stats, "interaction runs")?;
            migrate_optional_run(search.get_mut("confirmation_run"), stats)?;
        }
    }

    object.insert(
        "schema_version".to_string(),
        Value::from(CHECK_REPORT_SCHEMA_CURRENT),
    );
    Ok(())
}

fn migrate_fix_report(value: &mut Value, stats: &mut MigrationStats) -> Result<()> {
    let version = schema_version(value, EvidenceKind::FixReport)?;
    validate_schema_version(EvidenceKind::FixReport, version)?;
    let object = object_mut(value, "fix report")?;
    let diagnosis = object
        .get_mut("diagnosis")
        .context("fix report is missing diagnosis")?;
    migrate_check_report(diagnosis, stats)?;

    if let Some(verifications) = object.get_mut("verifications").and_then(Value::as_array_mut) {
        for verification in verifications {
            let verification = object_mut(verification, "fix verification")?;
            migrate_run_array(
                verification.get_mut("baseline_runs"),
                stats,
                "fix baseline runs",
            )?;
            migrate_run_array(
                verification.get_mut("intervention_runs"),
                stats,
                "fix intervention runs",
            )?;
        }
    }

    object.insert(
        "schema_version".to_string(),
        Value::from(FIX_REPORT_SCHEMA_CURRENT),
    );
    Ok(())
}

fn migrate_environment_comparison(value: &mut Value, stats: &mut MigrationStats) -> Result<()> {
    let version = schema_version(value, EvidenceKind::EnvironmentComparisonReport)?;
    validate_schema_version(EvidenceKind::EnvironmentComparisonReport, version)?;
    let object = object_mut(value, "environment comparison report")?;

    migrate_run_array(object.get_mut("good_runs"), stats, "good endpoint runs")?;
    migrate_failure_array(
        object.get_mut("good_failures"),
        stats,
        "good endpoint failures",
    )?;
    migrate_run_array(object.get_mut("bad_runs"), stats, "bad endpoint runs")?;
    migrate_failure_array(
        object.get_mut("bad_failures"),
        stats,
        "bad endpoint failures",
    )?;
    migrate_run_array(
        object.get_mut("reproduction_runs"),
        stats,
        "comparison reproduction runs",
    )?;
    migrate_failure_array(
        object.get_mut("reproduction_failures"),
        stats,
        "comparison reproduction failures",
    )?;
    migrate_optional_run(object.get_mut("confirmation_run"), stats)?;
    migrate_optional_failure(object.get_mut("confirmation_failure"), stats)?;

    object.insert(
        "schema_version".to_string(),
        Value::from(ENVIRONMENT_COMPARISON_SCHEMA_CURRENT),
    );
    Ok(())
}

fn migrate_run_array(value: Option<&mut Value>, stats: &mut MigrationStats, context: &str) -> Result<()> {
    let Some(value) = value else { return Ok(()); };
    let runs = value
        .as_array_mut()
        .with_context(|| format!("{context} must be an array"))?;
    for run in runs {
        migrate_build_run(run, stats)?;
    }
    Ok(())
}

fn migrate_failure_array(
    value: Option<&mut Value>,
    stats: &mut MigrationStats,
    context: &str,
) -> Result<()> {
    let Some(value) = value else { return Ok(()); };
    let failures = value
        .as_array_mut()
        .with_context(|| format!("{context} must be an array"))?;
    for failure in failures {
        migrate_build_failure(failure, stats)?;
    }
    Ok(())
}

fn migrate_optional_run(value: Option<&mut Value>, stats: &mut MigrationStats) -> Result<()> {
    if let Some(value) = value {
        if !value.is_null() {
            migrate_build_run(value, stats)?;
        }
    }
    Ok(())
}

fn migrate_optional_failure(value: Option<&mut Value>, stats: &mut MigrationStats) -> Result<()> {
    if let Some(value) = value {
        if !value.is_null() {
            migrate_build_failure(value, stats)?;
        }
    }
    Ok(())
}

fn migrate_build_run(value: &mut Value, stats: &mut MigrationStats) -> Result<()> {
    migrate_build_common(value, EvidenceKind::BuildRun, stats)?;
    let object = object_mut(value, "build run")?;
    if !object.contains_key("artifacts") {
        bail!("build run is missing artifacts");
    }
    Ok(())
}

fn migrate_build_failure(value: &mut Value, stats: &mut MigrationStats) -> Result<()> {
    migrate_build_common(value, EvidenceKind::BuildFailure, stats)
}

fn migrate_build_common(
    value: &mut Value,
    kind: EvidenceKind,
    stats: &mut MigrationStats,
) -> Result<()> {
    let source_version = schema_version(value, kind)?;
    validate_schema_version(kind, source_version)?;
    let object = object_mut(value, kind.label())?;

    validate_build_shape_for_version(object, kind, source_version)?;

    if source_version < BUILD_EVIDENCE_SCHEMA_CURRENT {
        stats.build_documents_migrated += 1;
    }

    if let Some(runtime) = object.get_mut("runtime_dependency_provenance") {
        migrate_runtime_dependency_provenance(runtime, source_version, stats)?;
    }

    if source_version < 10 {
        let stdout = object
            .get("stdout")
            .and_then(Value::as_str)
            .context("legacy build evidence is missing stdout text")?
            .as_bytes()
            .to_vec();
        let stderr = object
            .get("stderr")
            .and_then(Value::as_str)
            .context("legacy build evidence is missing stderr text")?
            .as_bytes()
            .to_vec();
        object.insert("log_capture_max_bytes".to_string(), Value::from(0_u64));
        object.insert("stdout_sha256".to_string(), Value::from(sha256_bytes(&stdout)));
        object.insert("stderr_sha256".to_string(), Value::from(sha256_bytes(&stderr)));
        object.insert("stdout_bytes".to_string(), Value::from(stdout.len() as u64));
        object.insert("stderr_bytes".to_string(), Value::from(stderr.len() as u64));
        object.insert("stdout_truncated".to_string(), Value::from(false));
        object.insert("stderr_truncated".to_string(), Value::from(false));
        stats.derived_legacy_log_digests += 1;
    }

    object.insert(
        "schema_version".to_string(),
        Value::from(BUILD_EVIDENCE_SCHEMA_CURRENT),
    );
    Ok(())
}

fn validate_build_shape_for_version(
    object: &Map<String, Value>,
    kind: EvidenceKind,
    source_version: u32,
) -> Result<()> {
    let require = |field: &str| -> Result<()> {
        if object.contains_key(field) {
            Ok(())
        } else {
            bail!(
                "{} schema {} is malformed: required field `{}` is missing",
                kind.label(),
                source_version,
                field
            )
        }
    };

    if source_version >= 2 {
        require("source_overrides")?;
        require("network_trace")?;
    }
    if source_version >= 3 {
        require("runtime_dependency_provenance")?;
    }
    if source_version >= 5 {
        require("process_trace")?;
    }
    if source_version >= 8 {
        require("runner_backend")?;
    }
    if source_version >= 10 {
        for field in [
            "log_capture_max_bytes",
            "stdout_sha256",
            "stderr_sha256",
            "stdout_bytes",
            "stderr_bytes",
            "stdout_truncated",
            "stderr_truncated",
        ] {
            require(field)?;
        }
    }

    Ok(())
}

fn migrate_runtime_dependency_provenance(
    value: &mut Value,
    source_version: u32,
    stats: &mut MigrationStats,
) -> Result<()> {
    let Some(object) = value.as_object_mut() else {
        bail!("runtime_dependency_provenance must be an object");
    };
    let Some(cache_summaries) = object.get_mut("cache_summaries") else {
        return Ok(());
    };
    let cache_summaries = cache_summaries
        .as_array_mut()
        .context("runtime dependency cache_summaries must be an array")?;

    for cache in cache_summaries {
        let cache = object_mut(cache, "dependency cache summary")?;
        let bounded_fields = [
            "before_truncated",
            "after_truncated",
            "max_files",
            "max_bytes",
            "observed_change",
            "comparison_complete",
        ];
        let missing_bounded = bounded_fields
            .iter()
            .filter(|field| !cache.contains_key(**field))
            .copied()
            .collect::<Vec<_>>();

        if !missing_bounded.is_empty() {
            if source_version >= 4 {
                bail!(
                    "build evidence schema {} has a malformed bounded dependency-cache summary; missing field(s): {}",
                    source_version,
                    missing_bounded.join(", ")
                );
            }

            cache.entry("before_truncated".to_string()).or_insert(Value::Bool(false));
            cache.entry("after_truncated".to_string()).or_insert(Value::Bool(false));
            cache.entry("max_files".to_string()).or_insert(Value::from(0_u64));
            cache.entry("max_bytes".to_string()).or_insert(Value::from(0_u64));
            let changed = cache
                .get("changed_during_build")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            cache
                .entry("observed_change".to_string())
                .or_insert(Value::Bool(changed));
            cache
                .entry("comparison_complete".to_string())
                .or_insert(Value::Bool(true));
            stats.legacy_cache_summaries_migrated += 1;
        }
    }
    Ok(())
}

fn object_mut<'a>(value: &'a mut Value, context: &str) -> Result<&'a mut Map<String, Value>> {
    value
        .as_object_mut()
        .with_context(|| format!("{context} must be a JSON object"))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const EXPERIMENT: &str = "00000000-0000-4000-8000-000000000001";
    const RUN: &str = "00000000-0000-4000-8000-000000000002";

    fn phase6_environment() -> Value {
        json!({
            "image_override": null,
            "container_source_path": "/src",
            "container_work_path": "/workspace",
            "source_copy_order": "sorted",
            "environment": {},
            "hostname": "reprobisect-control",
            "cpu_count": null,
            "source_mtime_epoch": null,
            "umask": null,
            "network_mode": "default"
        })
    }

    fn phase6_run() -> Value {
        json!({
            "schema_version": 1,
            "experiment_id": EXPERIMENT,
            "run_id": RUN,
            "ordinal": 1,
            "source_digest": "abc",
            "image": "example:latest",
            "resolved_image_id": "sha256:example",
            "command": ["true"],
            "working_directory": ".",
            "effective_environment": {},
            "controlled_environment": phase6_environment(),
            "toolchain_provenance": {"probes": [], "error": null},
            "exit_code": 0,
            "duration_ms": 1,
            "stdout": "legacy stdout\n",
            "stderr": "legacy stderr\n",
            "artifacts": [{
                "logical_path": "out",
                "sha256": "deadbeef",
                "size_bytes": 1,
                "detected_type": "generic",
                "embedded_markers": [],
                "elf_debug_markers": [],
                "elf_marker_locations": [],
                "elf_metadata": null,
                "archive_metadata": null
            }]
        })
    }

    fn phase6_check() -> Value {
        json!({
            "schema_version": 5,
            "experiment_id": EXPERIMENT,
            "status": "reproducible",
            "source_digest": "abc",
            "source_provenance": {"git_commit": null, "git_dirty": null, "dependency_files": []},
            "baseline_environment": phase6_environment(),
            "runs": [phase6_run()],
            "artifact_comparisons": [],
            "interventions": [],
            "interaction_search": null,
            "diagnoses": [],
            "notes": []
        })
    }

    #[test]
    fn phase6_check_normalizes_to_current_and_derives_full_log_identity() {
        let mut value = phase6_check();
        let mut stats = MigrationStats::default();
        migrate_check_report(&mut value, &mut stats).unwrap();
        let report: CheckReport = serde_json::from_value(value).unwrap();
        assert_eq!(report.schema_version, CHECK_REPORT_SCHEMA_CURRENT);
        assert_eq!(report.runs[0].schema_version, BUILD_EVIDENCE_SCHEMA_CURRENT);
        assert_eq!(report.runs[0].runner_backend, crate::model::RunnerBackend::Docker);
        assert_eq!(report.runs[0].stdout_bytes, 14);
        assert_eq!(
            report.runs[0].stdout_sha256,
            sha256_bytes(b"legacy stdout\n")
        );
        assert!(!report.runs[0].stdout_truncated);
        assert_eq!(stats.derived_legacy_log_digests, 1);
    }

    #[test]
    fn phase8_unbounded_cache_summary_gets_explicit_legacy_sentinels() {
        let mut run = phase6_run();
        run["schema_version"] = Value::from(3_u64);
        run["source_overrides"] = json!([]);
        run["network_trace"] = json!({
            "attempted": false,
            "tracer_available": false,
            "connect_calls": 0,
            "sendto_calls": 0
        });
        run["runtime_dependency_provenance"] = json!({
            "attempted": true,
            "dependency_files": [],
            "dependency_resolutions": [],
            "cache_summaries": [{
                "ecosystem": "cargo",
                "before_file_count": 1,
                "before_total_bytes": 10,
                "before_aggregate_sha256": "a",
                "after_file_count": 2,
                "after_total_bytes": 20,
                "after_aggregate_sha256": "b",
                "changed_during_build": true
            }],
            "error": null
        });
        let mut stats = MigrationStats::default();
        migrate_build_run(&mut run, &mut stats).unwrap();
        let run: BuildRun = serde_json::from_value(run).unwrap();
        let cache = &run.runtime_dependency_provenance.cache_summaries[0];
        assert_eq!(cache.max_files, 0);
        assert_eq!(cache.max_bytes, 0);
        assert!(cache.observed_change);
        assert!(cache.comparison_complete);
        assert_eq!(stats.legacy_cache_summaries_migrated, 1);
    }

    #[test]
    fn rejects_pre_phase6_and_future_schema_versions() {
        assert!(validate_schema_version(EvidenceKind::CheckReport, 4).is_err());
        assert!(validate_schema_version(EvidenceKind::CheckReport, 15).is_err());
        assert!(validate_schema_version(EvidenceKind::BuildRun, 0).is_err());
        assert!(validate_schema_version(EvidenceKind::BuildRun, 11).is_err());
        assert!(validate_schema_version(EvidenceKind::FixReport, 1).is_err());
        assert!(validate_schema_version(EvidenceKind::FixReport, 13).is_err());
        assert!(validate_schema_version(EvidenceKind::EnvironmentComparisonReport, 0).is_err());
        assert!(validate_schema_version(EvidenceKind::EnvironmentComparisonReport, 5).is_err());
    }

    #[test]
    fn kind_detection_is_structural_and_refuses_unknown_json() {
        assert_eq!(detect_kind(&phase6_check()).unwrap(), EvidenceKind::CheckReport);
        assert_eq!(detect_kind(&phase6_run()).unwrap(), EvidenceKind::BuildRun);
        assert!(detect_kind(&json!({"schema_version": 1, "hello": "world"})).is_err());
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("evidence")
            .join(name)
    }

    #[test]
    fn loads_frozen_phase6_check_fixture_end_to_end() {
        let loaded = load_evidence(&fixture("phase6-check-v5.json")).unwrap();
        assert_eq!(loaded.summary.kind, EvidenceKind::CheckReport);
        assert_eq!(loaded.summary.source_schema_version, 5);
        assert_eq!(loaded.summary.normalized_schema_version, CHECK_REPORT_SCHEMA_CURRENT);
        assert!(loaded.summary.migrated);
        assert_eq!(loaded.summary.build_documents_migrated, 1);
        assert_eq!(loaded.summary.derived_legacy_log_digests, 1);
        let EvidenceDocument::CheckReport(report) = loaded.document else {
            panic!("expected check report");
        };
        assert_eq!(report.runs[0].schema_version, BUILD_EVIDENCE_SCHEMA_CURRENT);
        assert_eq!(report.runs[0].stdout_sha256, sha256_bytes(b"legacy stdout\n"));
    }

    #[test]
    fn loads_frozen_phase6_build_failure_fixture_end_to_end() {
        let loaded = load_evidence(&fixture("phase6-build-failure-v1.json")).unwrap();
        assert_eq!(loaded.summary.kind, EvidenceKind::BuildFailure);
        assert_eq!(loaded.summary.source_schema_version, 1);
        assert_eq!(loaded.summary.derived_legacy_log_digests, 1);
        let EvidenceDocument::BuildFailure(failure) = loaded.document else {
            panic!("expected build failure");
        };
        assert_eq!(failure.schema_version, BUILD_EVIDENCE_SCHEMA_CURRENT);
        assert_eq!(
            failure.stdout_sha256,
            sha256_bytes(b"legacy failure stdout\n")
        );
    }

    #[test]
    fn loads_frozen_phase7_fix_fixture_end_to_end() {
        let loaded = load_evidence(&fixture("phase7-fix-v3.json")).unwrap();
        assert_eq!(loaded.summary.kind, EvidenceKind::FixReport);
        assert_eq!(loaded.summary.source_schema_version, 3);
        let EvidenceDocument::FixReport(report) = loaded.document else {
            panic!("expected fix report");
        };
        assert_eq!(report.schema_version, FIX_REPORT_SCHEMA_CURRENT);
        assert_eq!(report.diagnosis.schema_version, CHECK_REPORT_SCHEMA_CURRENT);
    }

    #[test]
    fn loads_frozen_phase8_cache_fixture_end_to_end() {
        let loaded = load_evidence(&fixture("phase8-build-run-v3.json")).unwrap();
        assert_eq!(loaded.summary.kind, EvidenceKind::BuildRun);
        assert_eq!(loaded.summary.source_schema_version, 3);
        assert_eq!(loaded.summary.legacy_cache_summaries_migrated, 1);
        let EvidenceDocument::BuildRun(run) = loaded.document else {
            panic!("expected build run");
        };
        let cache = &run.runtime_dependency_provenance.cache_summaries[0];
        assert_eq!(cache.max_files, 0);
        assert_eq!(cache.max_bytes, 0);
        assert!(cache.comparison_complete);
        assert!(cache.observed_change);
    }

    #[test]
    fn loads_phase14_comparison_fixture_and_defaults_failure_fields() {
        let loaded = load_evidence(&fixture("phase14-comparison-v1.json")).unwrap();
        assert_eq!(loaded.summary.kind, EvidenceKind::EnvironmentComparisonReport);
        assert_eq!(loaded.summary.source_schema_version, 1);
        let EvidenceDocument::EnvironmentComparisonReport(report) = loaded.document else {
            panic!("expected comparison report");
        };
        assert_eq!(report.schema_version, ENVIRONMENT_COMPARISON_SCHEMA_CURRENT);
        assert!(report.good_failures.is_empty());
        assert!(report.bad_failures.is_empty());
        assert!(report.reproduction_failures.is_empty());
        assert!(report.bad_failure_signature.is_none());
    }

    #[test]
    fn current_build_schema_refuses_missing_phase18_log_identity_fields() {
        let mut run = phase6_run();
        run["schema_version"] = Value::from(BUILD_EVIDENCE_SCHEMA_CURRENT);
        run["source_overrides"] = json!([]);
        run["network_trace"] = json!({});
        run["runtime_dependency_provenance"] = json!({});
        run["process_trace"] = json!({});
        run["runner_backend"] = Value::from("docker");

        let mut stats = MigrationStats::default();
        let error = migrate_build_run(&mut run, &mut stats).unwrap_err().to_string();
        assert!(error.contains("log_capture_max_bytes"));
    }

    #[test]
    fn schema8_build_evidence_refuses_missing_runner_backend() {
        let mut run = phase6_run();
        run["schema_version"] = Value::from(8_u64);
        run["source_overrides"] = json!([]);
        run["network_trace"] = json!({});
        run["runtime_dependency_provenance"] = json!({});
        run["process_trace"] = json!({});

        let mut stats = MigrationStats::default();
        let error = migrate_build_run(&mut run, &mut stats).unwrap_err().to_string();
        assert!(error.contains("runner_backend"));
    }

    #[test]
    fn schema4_cache_summary_refuses_missing_bounded_fields() {
        let mut run = phase6_run();
        run["schema_version"] = Value::from(4_u64);
        run["source_overrides"] = json!([]);
        run["network_trace"] = json!({});
        run["runtime_dependency_provenance"] = json!({
            "attempted": true,
            "cache_summaries": [{
                "ecosystem": "cargo",
                "before_file_count": 1,
                "before_total_bytes": 10,
                "after_file_count": 2,
                "after_total_bytes": 20,
                "changed_during_build": true
            }]
        });

        let mut stats = MigrationStats::default();
        let error = migrate_build_run(&mut run, &mut stats).unwrap_err().to_string();
        assert!(error.contains("malformed bounded dependency-cache summary"));
    }

    #[test]
    fn current_phase18_build_fixture_requires_no_migration() {
        let loaded = load_evidence(&fixture("phase18-build-run-v10.json")).unwrap();
        assert_eq!(loaded.summary.kind, EvidenceKind::BuildRun);
        assert_eq!(loaded.summary.source_schema_version, BUILD_EVIDENCE_SCHEMA_CURRENT);
        assert!(!loaded.summary.migrated);
        assert_eq!(loaded.summary.build_documents_migrated, 0);
        assert_eq!(loaded.summary.derived_legacy_log_digests, 0);
    }

    #[test]
    fn normalized_write_is_create_only_without_force() {
        let temp = tempfile::tempdir().unwrap();
        let mut value = phase6_run();
        let mut stats = MigrationStats::default();
        migrate_build_run(&mut value, &mut stats).unwrap();
        let run: BuildRun = serde_json::from_value(value).unwrap();
        let document = EvidenceDocument::BuildRun(run);
        let source = temp.path().join("source.json");
        fs::write(&source, b"{}\n").unwrap();
        let path = temp.path().join("normalized.json");
        write_normalized(&source, &path, &document, false).unwrap();
        assert!(write_normalized(&source, &path, &document, false).is_err());
        write_normalized(&source, &path, &document, true).unwrap();
        assert!(write_normalized(&source, &source, &document, true).is_err());
    }
}
