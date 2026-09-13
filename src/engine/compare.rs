use std::path::Path;

use anyhow::{Result, bail};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    config::Config,
    environment::{LoadedEnvironmentManifest, apply_delta_subset, diff_environments},
    evidence::{persist_build_failure, persist_environment_comparison, persist_run},
    model::{
        BuildFailure, BuildFailureSignature, BuildRun, BuildSpec, EnvironmentComparisonOutcomeKind,
        EnvironmentComparisonReport, EnvironmentComparisonStatus, EnvironmentDeltaRecord,
    },
    runner::{RunOutcome, Runner, configured_runner},
    schema::{BUILD_EVIDENCE_SCHEMA_CURRENT, ENVIRONMENT_COMPARISON_SCHEMA_CURRENT},
    source::{collect_source_provenance, digest_tree},
};

use super::{
    control::{build_artifact_deltas, distinct_artifact_outcomes, same_artifact_hashes},
    ddmin,
    interventions::baseline_environment,
};

#[derive(Debug, Clone)]
pub struct CompareOptions {
    pub endpoint_runs: usize,
    pub subset_runs: usize,
    pub confirmation_runs: usize,
}

type ComparisonReport = EnvironmentComparisonReport;

#[derive(Debug, Default)]
struct EndpointAttempts {
    runs: Vec<BuildRun>,
    failures: Vec<BuildFailure>,
}

#[derive(Debug, Clone)]
enum StableEndpointReference {
    Artifacts(BuildRun),
    Failure(BuildFailureSignature),
}

pub fn compare_environments(
    project_root: &Path,
    config: &Config,
    good_manifest: &LoadedEnvironmentManifest,
    bad_manifest: &LoadedEnvironmentManifest,
    options: &CompareOptions,
) -> Result<EnvironmentComparisonReport> {
    validate_options(options)?;

    let experiment_id = Uuid::new_v4();
    let source_digest = digest_tree(project_root)?;
    let source_provenance = collect_source_provenance(project_root)?;
    let spec = BuildSpec {
        image: config.build.image.clone(),
        command: config.build.command.clone(),
        outputs: config.build.outputs.clone(),
        environment: config.build.env.clone(),
        working_directory: config.build.working_directory.clone(),
        timeout_seconds: config.build.timeout_seconds,
        log_capture_max_bytes: config.build.log_capture_max_bytes,
    };
    let runner = configured_runner(project_root, config.build.runner);
    runner.available()?;

    let canonical = baseline_environment(config);
    let good_environment = good_manifest.manifest.apply(&canonical, config)?;
    let bad_environment = bad_manifest.manifest.apply(&canonical, config)?;
    let delta = diff_environments(&good_environment, &bad_environment);
    let delta_records = delta
        .iter()
        .map(|entry| entry.record.clone())
        .collect::<Vec<_>>();

    let mut ordinal = 1_usize;
    let good_attempts = run_environment_attempts(
        project_root,
        &runner,
        &spec,
        experiment_id,
        &source_digest,
        &good_environment,
        options.endpoint_runs,
        &mut ordinal,
    )?;
    let bad_attempts = run_environment_attempts(
        project_root,
        &runner,
        &spec,
        experiment_id,
        &source_digest,
        &bad_environment,
        options.endpoint_runs,
        &mut ordinal,
    )?;

    let good_outcome_kind = endpoint_outcome_kind(&good_attempts);
    let bad_outcome_kind = endpoint_outcome_kind(&bad_attempts);
    let good_reference = stable_endpoint_reference(&good_attempts, options.endpoint_runs);
    let bad_reference = stable_endpoint_reference(&bad_attempts, options.endpoint_runs);
    let artifact_deltas = match (good_attempts.runs.first(), bad_attempts.runs.first()) {
        (Some(good), Some(_)) => build_artifact_deltas(good, &bad_attempts.runs, None),
        _ => Vec::new(),
    };
    let bad_failure_signature = match &bad_reference {
        Some(StableEndpointReference::Failure(signature)) => Some(signature.clone()),
        _ => None,
    };

    if good_reference.is_none() || bad_reference.is_none() {
        let mut notes = Vec::new();
        if good_reference.is_none() {
            notes.push(
                "known-good endpoint did not produce one stable repeated outcome; successes/failures or artifact/failure signatures varied"
                    .to_string(),
            );
        }
        if bad_reference.is_none() {
            notes.push(
                "known-bad endpoint did not produce one stable repeated outcome; successes/failures or artifact/failure signatures varied"
                    .to_string(),
            );
        }
        notes.push(
            "delta minimization was not attempted because ddmin requires a stable reproduction predicate"
                .to_string(),
        );
        return finish_report(
            project_root,
            report_from_attempts(
                experiment_id,
                EnvironmentComparisonStatus::Inconclusive,
                source_digest,
                source_provenance,
                good_manifest,
                bad_manifest,
                good_outcome_kind,
                bad_outcome_kind,
                good_attempts,
                bad_attempts,
                bad_failure_signature,
                artifact_deltas,
                delta_records,
                Vec::new(),
                0,
                EndpointAttempts::default(),
                None,
                None,
                None,
                notes,
            ),
        );
    }

    let good_reference = good_reference.expect("checked above");
    let bad_reference = bad_reference.expect("checked above");
    let good_artifact_reference = match &good_reference {
        StableEndpointReference::Artifacts(run) => run.clone(),
        StableEndpointReference::Failure(_) => {
            return finish_report(
                project_root,
                report_from_attempts(
                    experiment_id,
                    EnvironmentComparisonStatus::Inconclusive,
                    source_digest,
                    source_provenance,
                    good_manifest,
                    bad_manifest,
                    good_outcome_kind,
                    bad_outcome_kind,
                    good_attempts,
                    bad_attempts,
                    bad_failure_signature,
                    artifact_deltas,
                    delta_records,
                    Vec::new(),
                    0,
                    EndpointAttempts::default(),
                    None,
                    None,
                    None,
                    vec![
                        "known-good endpoint stably exits non-zero; Phase 16 requires known-good to produce the declared artifacts successfully"
                            .to_string(),
                    ],
                ),
            );
        }
    };

    if let StableEndpointReference::Artifacts(bad_artifact_reference) = &bad_reference {
        if same_artifact_hashes(&good_artifact_reference, bad_artifact_reference) {
            return finish_report(
                project_root,
                report_from_attempts(
                    experiment_id,
                    EnvironmentComparisonStatus::Equivalent,
                    source_digest,
                    source_provenance,
                    good_manifest,
                    bad_manifest,
                    good_outcome_kind,
                    bad_outcome_kind,
                    good_attempts,
                    bad_attempts,
                    None,
                    artifact_deltas,
                    delta_records,
                    Vec::new(),
                    0,
                    EndpointAttempts::default(),
                    None,
                    None,
                    None,
                    vec![
                        "known-good and known-bad manifests produced the same stable declared artifacts"
                            .to_string(),
                    ],
                ),
            );
        }
    }

    if delta.is_empty() {
        let difference = match &bad_reference {
            StableEndpointReference::Artifacts(_) => "stable endpoint artifacts differ",
            StableEndpointReference::Failure(_) => {
                "known-good succeeds while known-bad has a stable build-failure signature"
            }
        };
        return finish_report(
            project_root,
            report_from_attempts(
                experiment_id,
                EnvironmentComparisonStatus::Inconclusive,
                source_digest,
                source_provenance,
                good_manifest,
                bad_manifest,
                good_outcome_kind,
                bad_outcome_kind,
                good_attempts,
                bad_attempts,
                bad_failure_signature,
                artifact_deltas,
                Vec::new(),
                Vec::new(),
                0,
                EndpointAttempts::default(),
                None,
                None,
                None,
                vec![
                    format!("{difference}, but the supported controlled environment delta is empty"),
                    "the difference is outside the environment-manifest dimensions modeled by this command"
                        .to_string(),
                ],
            ),
        );
    }

    let outcome = ddmin::minimize(&delta, |subset| {
        let candidate_environment = apply_delta_subset(&good_environment, subset);
        let attempts = run_environment_attempts(
            project_root,
            &runner,
            &spec,
            experiment_id,
            &source_digest,
            &candidate_environment,
            options.subset_runs,
            &mut ordinal,
        )?;
        Ok(attempts_reproduce_reference(&attempts, &bad_reference))
    })?;

    let minimal_environment = apply_delta_subset(&good_environment, &outcome.minimal);
    let reproduction_attempts = run_environment_attempts(
        project_root,
        &runner,
        &spec,
        experiment_id,
        &source_digest,
        &minimal_environment,
        options.subset_runs,
        &mut ordinal,
    )?;
    let reproduction_stable =
        attempts_reproduce_reference(&reproduction_attempts, &bad_reference);

    let mut confirmation_run = None;
    let mut confirmation_failure = None;
    let mut reverted_to_good = None;
    if options.confirmation_runs == 1 {
        match runner.run_attempt(
            &spec,
            experiment_id,
            &source_digest,
            ordinal,
            &good_environment,
        )? {
            RunOutcome::Success(run) => {
                persist_run(project_root, &run)?;
                reverted_to_good = Some(same_artifact_hashes(&good_artifact_reference, &run));
                confirmation_run = Some(run);
            }
            RunOutcome::BuildFailed(failure) => {
                persist_build_failure(project_root, &failure)?;
                reverted_to_good = Some(false);
                confirmation_failure = Some(failure);
            }
        }
    }

    let minimal_delta = outcome
        .minimal
        .iter()
        .map(|entry| entry.record.clone())
        .collect::<Vec<_>>();
    let confirmation_ok = reverted_to_good != Some(false);
    let status = if reproduction_stable && confirmation_ok {
        EnvironmentComparisonStatus::Minimized
    } else {
        EnvironmentComparisonStatus::Inconclusive
    };
    let predicate_note = match &bad_reference {
        StableEndpointReference::Artifacts(_) => {
            "subset reproduction requires the exact stable known-bad declared-artifact signature, not merely any difference from known-good"
        }
        StableEndpointReference::Failure(_) => {
            "subset reproduction requires the exact stable known-bad build-failure signature: exit code plus SHA-256 of stdout and stderr"
        }
    };
    let mut notes = vec![
        "minimal_delta is 1-minimal under the tested ddmin search path, values, and repetition policy; it is not a proof of global real-world minimality".to_string(),
        predicate_note.to_string(),
    ];
    if matches!(bad_reference, StableEndpointReference::Failure(_)) {
        notes.push(
            "matching a build-failure signature establishes reproduction of the observed process outcome, not semantic identity of the underlying defect"
                .to_string(),
        );
    }
    if !reproduction_stable {
        notes.push(
            "the final minimized subset did not stably reproduce the known-bad outcome on confirmation"
                .to_string(),
        );
    }
    if reverted_to_good == Some(false) {
        notes.push(
            "known-good reversion did not recover the original successful good artifact signature; temporal drift or an uncontrolled input may be present"
                .to_string(),
        );
    }

    finish_report(
        project_root,
        report_from_attempts(
            experiment_id,
            status,
            source_digest,
            source_provenance,
            good_manifest,
            bad_manifest,
            good_outcome_kind,
            bad_outcome_kind,
            good_attempts,
            bad_attempts,
            bad_failure_signature,
            artifact_deltas,
            delta_records,
            minimal_delta,
            outcome.tests + 1,
            reproduction_attempts,
            reverted_to_good,
            confirmation_run,
            confirmation_failure,
            notes,
        ),
    )
}

#[allow(clippy::too_many_arguments)]
fn report_from_attempts(
    experiment_id: Uuid,
    status: EnvironmentComparisonStatus,
    source_digest: String,
    source_provenance: crate::model::SourceProvenance,
    good_manifest: &LoadedEnvironmentManifest,
    bad_manifest: &LoadedEnvironmentManifest,
    good_outcome_kind: EnvironmentComparisonOutcomeKind,
    bad_outcome_kind: EnvironmentComparisonOutcomeKind,
    good_attempts: EndpointAttempts,
    bad_attempts: EndpointAttempts,
    bad_failure_signature: Option<BuildFailureSignature>,
    artifact_deltas: Vec<crate::model::ArtifactDelta>,
    delta: Vec<EnvironmentDeltaRecord>,
    minimal_delta: Vec<EnvironmentDeltaRecord>,
    tested_subsets: usize,
    reproduction_attempts: EndpointAttempts,
    reverted_to_good: Option<bool>,
    confirmation_run: Option<BuildRun>,
    confirmation_failure: Option<BuildFailure>,
    notes: Vec<String>,
) -> ComparisonReport {
    EnvironmentComparisonReport {
        schema_version: ENVIRONMENT_COMPARISON_SCHEMA_CURRENT,
        experiment_id,
        status,
        source_digest,
        source_provenance,
        good_manifest_sha256: good_manifest.sha256.clone(),
        bad_manifest_sha256: bad_manifest.sha256.clone(),
        good_outcome_kind,
        bad_outcome_kind,
        good_runs: good_attempts.runs,
        good_failures: good_attempts.failures,
        bad_runs: bad_attempts.runs,
        bad_failures: bad_attempts.failures,
        bad_failure_signature,
        artifact_deltas,
        delta,
        minimal_delta,
        tested_subsets,
        reproduction_runs: reproduction_attempts.runs,
        reproduction_failures: reproduction_attempts.failures,
        reverted_to_good,
        confirmation_run,
        confirmation_failure,
        notes,
    }
}

fn validate_options(options: &CompareOptions) -> Result<()> {
    if !(2..=32).contains(&options.endpoint_runs) {
        bail!("compare endpoint_runs must be between 2 and 32");
    }
    if !(2..=32).contains(&options.subset_runs) {
        bail!("compare subset_runs must be between 2 and 32");
    }
    if options.confirmation_runs > 1 {
        bail!("compare confirmation_runs currently supports only 0 or 1");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_environment_attempts<R: Runner>(
    project_root: &Path,
    runner: &R,
    spec: &BuildSpec,
    experiment_id: Uuid,
    source_digest: &str,
    environment: &crate::model::ControlledEnvironment,
    run_count: usize,
    ordinal: &mut usize,
) -> Result<EndpointAttempts> {
    let mut attempts = EndpointAttempts {
        runs: Vec::with_capacity(run_count),
        failures: Vec::with_capacity(run_count),
    };
    for _ in 0..run_count {
        match runner.run_attempt(
            spec,
            experiment_id,
            source_digest,
            *ordinal,
            environment,
        )? {
            RunOutcome::Success(run) => {
                persist_run(project_root, &run)?;
                attempts.runs.push(run);
            }
            RunOutcome::BuildFailed(failure) => {
                persist_build_failure(project_root, &failure)?;
                attempts.failures.push(failure);
            }
        }
        *ordinal += 1;
    }
    Ok(attempts)
}

fn endpoint_outcome_kind(attempts: &EndpointAttempts) -> EnvironmentComparisonOutcomeKind {
    match (attempts.runs.is_empty(), attempts.failures.is_empty()) {
        (false, true) => EnvironmentComparisonOutcomeKind::ArtifactSuccess,
        (true, false) => EnvironmentComparisonOutcomeKind::BuildFailure,
        _ => EnvironmentComparisonOutcomeKind::Mixed,
    }
}

fn stable_endpoint_reference(
    attempts: &EndpointAttempts,
    expected_runs: usize,
) -> Option<StableEndpointReference> {
    if attempts.failures.is_empty()
        && attempts.runs.len() == expected_runs
        && distinct_artifact_outcomes(&attempts.runs) == 1
    {
        return attempts
            .runs
            .first()
            .cloned()
            .map(StableEndpointReference::Artifacts);
    }
    if attempts.runs.is_empty() && attempts.failures.len() == expected_runs {
        let first = failure_signature(attempts.failures.first()?);
        if attempts
            .failures
            .iter()
            .all(|failure| failure_signature(failure) == first)
        {
            return Some(StableEndpointReference::Failure(first));
        }
    }
    None
}

fn attempts_reproduce_reference(
    attempts: &EndpointAttempts,
    reference: &StableEndpointReference,
) -> bool {
    match reference {
        StableEndpointReference::Artifacts(reference_run) => {
            attempts.failures.is_empty()
                && runs_reproduce_reference(&attempts.runs, reference_run)
        }
        StableEndpointReference::Failure(reference_failure) => {
            attempts.runs.is_empty()
                && !attempts.failures.is_empty()
                && attempts
                    .failures
                    .iter()
                    .all(|failure| failure_signature(failure) == *reference_failure)
        }
    }
}

fn runs_reproduce_reference(runs: &[BuildRun], reference: &BuildRun) -> bool {
    !runs.is_empty()
        && distinct_artifact_outcomes(runs) == 1
        && runs.iter().all(|run| same_artifact_hashes(reference, run))
}

fn failure_signature(failure: &BuildFailure) -> BuildFailureSignature {
    BuildFailureSignature {
        exit_code: failure.exit_code,
        stdout_sha256: if failure.stdout_sha256.is_empty() {
            sha256_bytes(failure.stdout.as_bytes())
        } else {
            failure.stdout_sha256.clone()
        },
        stderr_sha256: if failure.stderr_sha256.is_empty() {
            sha256_bytes(failure.stderr.as_bytes())
        } else {
            failure.stderr_sha256.clone()
        },
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn finish_report(
    project_root: &Path,
    report: EnvironmentComparisonReport,
) -> Result<EnvironmentComparisonReport> {
    persist_environment_comparison(project_root, &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ArtifactRecord, ArtifactType, ControlledEnvironment, RunnerBackend,
    };
    use std::{collections::BTreeMap, path::PathBuf};

    fn run_with_hash(hash: &str) -> BuildRun {
        BuildRun {
            schema_version: BUILD_EVIDENCE_SCHEMA_CURRENT,
            experiment_id: Uuid::nil(),
            run_id: Uuid::new_v4(),
            ordinal: 1,
            runner_backend: RunnerBackend::Docker,
            source_digest: "source".to_string(),
            image: "demo".to_string(),
            resolved_image_id: "sha256:demo".to_string(),
            command: vec!["true".to_string()],
            working_directory: PathBuf::from("."),
            effective_environment: BTreeMap::new(),
            controlled_environment: ControlledEnvironment::default(),
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
            stdout_sha256: sha256_bytes(b""),
            stderr_sha256: sha256_bytes(b""),
            stdout_bytes: 0,
            stderr_bytes: 0,
            stdout_truncated: false,
            stderr_truncated: false,
            artifacts: vec![ArtifactRecord {
                logical_path: PathBuf::from("out"),
                sha256: hash.to_string(),
                size_bytes: 1,
                detected_type: ArtifactType::Generic,
                embedded_markers: Vec::new(),
                elf_debug_markers: Vec::new(),
                elf_marker_locations: Vec::new(),
                elf_metadata: None,
                archive_metadata: None,
                semantic_metadata: None,
            }],
        }
    }

    fn failure(exit_code: i32, stdout: &str, stderr: &str) -> BuildFailure {
        BuildFailure {
            schema_version: BUILD_EVIDENCE_SCHEMA_CURRENT,
            experiment_id: Uuid::nil(),
            run_id: Uuid::new_v4(),
            ordinal: 1,
            runner_backend: RunnerBackend::Docker,
            source_digest: "source".to_string(),
            image: "demo".to_string(),
            resolved_image_id: "sha256:demo".to_string(),
            command: vec!["false".to_string()],
            working_directory: PathBuf::from("."),
            effective_environment: BTreeMap::new(),
            controlled_environment: ControlledEnvironment::default(),
            toolchain_provenance: Default::default(),
            source_overrides: Vec::new(),
            network_trace: Default::default(),
            process_trace: Default::default(),
            runtime_dependency_provenance: Default::default(),
            exit_code,
            duration_ms: 1,
            log_capture_max_bytes: 1024 * 1024,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            stdout_sha256: sha256_bytes(stdout.as_bytes()),
            stderr_sha256: sha256_bytes(stderr.as_bytes()),
            stdout_bytes: stdout.len() as u64,
            stderr_bytes: stderr.len() as u64,
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }

    #[test]
    fn reproduction_requires_exact_bad_signature_and_stability() {
        let bad = run_with_hash("bad");
        assert!(runs_reproduce_reference(
            &[run_with_hash("bad"), run_with_hash("bad")],
            &bad
        ));
        assert!(!runs_reproduce_reference(
            &[run_with_hash("other"), run_with_hash("other")],
            &bad
        ));
        assert!(!runs_reproduce_reference(
            &[run_with_hash("bad"), run_with_hash("other")],
            &bad
        ));
    }

    #[test]
    fn failure_reproduction_requires_exact_exit_and_output_fingerprints() {
        let bad = failure(37, "", "known bad\n");
        let reference = StableEndpointReference::Failure(failure_signature(&bad));
        let exact = EndpointAttempts {
            runs: vec![],
            failures: vec![failure(37, "", "known bad\n"), failure(37, "", "known bad\n")],
        };
        assert!(attempts_reproduce_reference(&exact, &reference));

        let wrong_diagnostic = EndpointAttempts {
            runs: vec![],
            failures: vec![failure(37, "", "different\n"), failure(37, "", "different\n")],
        };
        assert!(!attempts_reproduce_reference(&wrong_diagnostic, &reference));

        let wrong_exit = EndpointAttempts {
            runs: vec![],
            failures: vec![failure(2, "", "known bad\n"), failure(2, "", "known bad\n")],
        };
        assert!(!attempts_reproduce_reference(&wrong_exit, &reference));
    }

    #[test]
    fn failure_signature_uses_full_stream_digest_when_retained_text_is_truncated() {
        let mut failure = failure(37, "prefix", "diagnostic-prefix");
        let full_stdout = b"prefix-and-unretained-tail";
        let full_stderr = b"diagnostic-prefix-and-unretained-tail";
        failure.stdout_sha256 = sha256_bytes(full_stdout);
        failure.stderr_sha256 = sha256_bytes(full_stderr);
        failure.stdout_truncated = true;
        failure.stderr_truncated = true;
        failure.stdout_bytes = full_stdout.len() as u64;
        failure.stderr_bytes = full_stderr.len() as u64;
        let signature = failure_signature(&failure);
        assert_eq!(signature.stdout_sha256, sha256_bytes(full_stdout));
        assert_eq!(signature.stderr_sha256, sha256_bytes(full_stderr));
    }

    #[test]
    fn mixed_success_and_failure_is_not_a_stable_endpoint() {
        let attempts = EndpointAttempts {
            runs: vec![run_with_hash("good")],
            failures: vec![failure(37, "", "known bad\n")],
        };
        assert!(stable_endpoint_reference(&attempts, 2).is_none());
        assert_eq!(
            endpoint_outcome_kind(&attempts),
            EnvironmentComparisonOutcomeKind::Mixed
        );
    }
}
