use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::{
    model::{BuildFailure, BuildRun, CheckReport, EnvironmentComparisonReport, RunnerBackend},
    schema::BUILD_EVIDENCE_SCHEMA_CURRENT,
};

pub fn persist_run(project_root: &Path, run: &BuildRun) -> Result<PathBuf> {
    let directory = project_root
        .join(".reprobisect")
        .join("runs")
        .join(run.experiment_id.to_string());
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create evidence directory {}", directory.display()))?;

    let path = directory.join(format!("run-{:04}-{}.json", run.ordinal, run.run_id));
    write_new_json(&path, run)?;
    Ok(path)
}


pub fn persist_build_failure(project_root: &Path, failure: &BuildFailure) -> Result<PathBuf> {
    let directory = project_root
        .join(".reprobisect")
        .join("runs")
        .join(failure.experiment_id.to_string());
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create evidence directory {}", directory.display()))?;

    let path = directory.join(format!(
        "run-{:04}-{}-failed.json",
        failure.ordinal, failure.run_id
    ));
    write_new_json(&path, failure)?;
    Ok(path)
}

pub fn persist_environment_comparison(
    project_root: &Path,
    report: &EnvironmentComparisonReport,
) -> Result<PathBuf> {
    let directory = project_root.join(".reprobisect").join("comparisons");
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create evidence directory {}", directory.display()))?;

    let path = directory.join(format!("{}.json", report.experiment_id));
    write_new_json(&path, report)?;
    Ok(path)
}

pub fn persist_report(project_root: &Path, report: &CheckReport) -> Result<PathBuf> {
    let directory = project_root.join(".reprobisect").join("experiments");
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create evidence directory {}", directory.display()))?;

    let path = directory.join(format!("{}.json", report.experiment_id));
    write_new_json(&path, report)?;
    Ok(path)
}

fn write_new_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).context("cannot serialize experiment evidence")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("cannot create immutable evidence file {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write evidence file {}", path.display()))?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use uuid::Uuid;

    use super::*;
    use crate::model::{ArtifactRecord, ArtifactType, ControlledEnvironment};

    #[test]
    fn run_manifest_is_create_only() {
        let temp = tempfile::tempdir().unwrap();
        let experiment_id = Uuid::new_v4();
        let run = BuildRun {
            schema_version: BUILD_EVIDENCE_SCHEMA_CURRENT,
            experiment_id,
            run_id: Uuid::new_v4(),
            ordinal: 1,
            runner_backend: RunnerBackend::Docker,
            source_digest: "abc".to_string(),
            image: "example:latest".to_string(),
            resolved_image_id: "sha256:example".to_string(),
            command: vec!["true".to_string()],
            working_directory: PathBuf::from("."),
            effective_environment: BTreeMap::new(),
            controlled_environment: ControlledEnvironment {
                hostname: Some("reprobisect-control".to_string()),
                ..ControlledEnvironment::default()
            },
            toolchain_provenance: Default::default(),
            source_overrides: Vec::new(),
            network_trace: Default::default(),
            process_trace: Default::default(),
            runtime_dependency_provenance: Default::default(),
            exit_code: 0,
            duration_ms: 1,
            log_capture_max_bytes: 1024 * 1024,
            stdout: String::new(),
            stderr: String::new(),
            stdout_sha256: String::new(),
            stderr_sha256: String::new(),
            stdout_bytes: 0,
            stderr_bytes: 0,
            stdout_truncated: false,
            stderr_truncated: false,
            artifacts: vec![ArtifactRecord {
                logical_path: PathBuf::from("out"),
                sha256: "deadbeef".to_string(),
                size_bytes: 1,
                detected_type: ArtifactType::Generic,
                embedded_markers: Vec::new(),
                elf_debug_markers: Vec::new(),
                elf_marker_locations: Vec::new(),
                elf_metadata: None,
                archive_metadata: None,
                semantic_metadata: None,
            }],
        };

        let path = persist_run(temp.path(), &run).unwrap();
        assert!(path.exists());
        assert!(persist_run(temp.path(), &run).is_err());
    }
    #[test]
    fn build_failure_manifest_is_create_only() {
        let temp = tempfile::tempdir().unwrap();
        let failure = BuildFailure {
            schema_version: BUILD_EVIDENCE_SCHEMA_CURRENT,
            experiment_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            ordinal: 1,
            runner_backend: RunnerBackend::Docker,
            source_digest: "abc".to_string(),
            image: "example:latest".to_string(),
            resolved_image_id: "sha256:example".to_string(),
            command: vec!["false".to_string()],
            working_directory: PathBuf::from("."),
            effective_environment: BTreeMap::new(),
            controlled_environment: ControlledEnvironment::default(),
            toolchain_provenance: Default::default(),
            source_overrides: Vec::new(),
            network_trace: Default::default(),
            process_trace: Default::default(),
            runtime_dependency_provenance: Default::default(),
            exit_code: 42,
            duration_ms: 1,
            log_capture_max_bytes: 1024 * 1024,
            stdout: String::new(),
            stderr: "expected failure".to_string(),
            stdout_sha256: String::new(),
            stderr_sha256: String::new(),
            stdout_bytes: 0,
            stderr_bytes: 16,
            stdout_truncated: false,
            stderr_truncated: false,
        };

        let path = persist_build_failure(temp.path(), &failure).unwrap();
        assert!(path.exists());
        assert!(persist_build_failure(temp.path(), &failure).is_err());
    }

}
