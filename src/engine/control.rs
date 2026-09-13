use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::{
    artifact::{compare_archive_metadata, compare_runs, compare_semantic_metadata},
    config::Config,
    engine::{
        ddmin,
        diagnosis::synthesize_diagnoses,
        interventions::{
            PlannedIntervention, baseline_environment, combine_interventions,
            eligible_for_interaction_search, plan_interventions,
        },
        statistics::assess_stochastic_effect,
    },
    evidence::{persist_build_failure, persist_report, persist_run},
    model::{
        ArchiveMetadataDifference, ArchiveMetadataField, ArtifactDelta, ArtifactSemanticDifference, BuildRun, BuildSpec,
        CheckReport, CheckStatus, ElfMarkerLocation, EmbeddedMarker, InteractionSearchResult, Intervention,
        InterventionKind, InterventionResult, StochasticEffectClassification,
    },
    runner::{RunOutcome, Runner, configured_runner},
    schema::CHECK_REPORT_SCHEMA_CURRENT,
    source::{collect_source_provenance, digest_tree},
};

#[derive(Debug, Clone)]
pub struct CheckOptions {
    pub control_runs: usize,
    pub intervention_runs: usize,
    pub confirmation_runs: usize,
    pub stochastic_runs: usize,
    pub stochastic_alpha: f64,
    pub interaction_runs: usize,
    pub max_interaction_variables: usize,
}

pub fn check_project(
    project_root: &Path,
    config: &Config,
    options: &CheckOptions,
) -> Result<CheckReport> {
    let experiment_id = Uuid::new_v4();
    let source_digest = digest_tree(project_root).context("failed to digest source tree")?;
    let source_provenance = collect_source_provenance(project_root)
        .context("failed to collect source/dependency provenance")?;
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

    let baseline = baseline_environment(config);
    let mut runs = Vec::with_capacity(options.control_runs);
    let mut ordinal = 1_usize;

    for _ in 0..options.control_runs {
        let run = runner.run(&spec, experiment_id, &source_digest, ordinal, &baseline)?;
        persist_run(project_root, &run)?;
        runs.push(run);
        ordinal += 1;
    }

    let artifact_comparisons = compare_runs(&runs);
    let controls_stable = artifact_comparisons
        .iter()
        .all(|comparison| comparison.equal_across_runs);

    if !controls_stable {
        let report = CheckReport {
            schema_version: CHECK_REPORT_SCHEMA_CURRENT,
            experiment_id,
            status: CheckStatus::UncontrolledNondeterminism,
            source_digest,
            source_provenance,
            baseline_environment: baseline,
            runs,
            artifact_comparisons,
            interventions: Vec::new(),
            interaction_search: None,
            diagnoses: Vec::new(),
            notes: vec![
                format!(
                    "declared artifacts differed across {} repetitions of the same controlled baseline",
                    options.control_runs
                ),
                "deterministic intervention attribution and ddmin interaction search were not attempted because the baseline is unstable".to_string(),
                "this establishes uncontrolled nondeterminism under the tested baseline, not a universal intrinsic-nondeterminism claim".to_string(),
            ],
        };
        persist_report(project_root, &report)?;
        return Ok(report);
    }

    let planned = plan_interventions(config, &baseline);
    let mut intervention_results = Vec::with_capacity(planned.len());
    for intervention in &planned {
        intervention_results.push(execute_intervention(
            project_root,
            &runner,
            &spec,
            experiment_id,
            &source_digest,
            &runs[0],
            &baseline,
            intervention.clone(),
            options,
            &mut ordinal,
        )?);
    }

    let singles_changed = intervention_results.iter().any(|result| result.changed);
    let build_outcome_dependency = intervention_results.iter().any(|result| {
        build_failure_is_observable(result.intervention.kind)
            && !result.build_failures.is_empty()
    });
    let late_baseline_instability = intervention_results.iter().any(|result| {
        result.stochastic_effect.as_ref().is_some_and(|evidence| {
            evidence.classification == StochasticEffectClassification::BaselineUnstable
        })
    });

    let eligible_candidates = planned
        .iter()
        .zip(intervention_results.iter())
        .filter(|(planned, result)| {
            eligible_for_interaction_search(planned) && !result.changed && result.error.is_none()
        })
        .map(|(planned, _)| planned.clone())
        .collect::<Vec<_>>();

    let mut interaction_search = None;
    let mut interaction_note = None;
    let mut interaction_failed = false;
    if !late_baseline_instability
        && config.experiments.interaction_search
        && eligible_candidates.len() >= 2
    {
        if eligible_candidates.len() <= options.max_interaction_variables {
            match execute_interaction_search(
                project_root,
                &runner,
                &spec,
                experiment_id,
                &source_digest,
                &runs[0],
                &baseline,
                &eligible_candidates,
                options,
                &mut ordinal,
            ) {
                Ok(result) => interaction_search = result,
                Err(error) => {
                    interaction_failed = true;
                    interaction_note = Some(format!(
                        "interaction search could not be completed and was excluded from positive reproducibility claims: {error:#}"
                    ));
                }
            }
        } else {
            interaction_note = Some(format!(
                "interaction search skipped: {} eligible variables exceed experiments.max_interaction_variables={}",
                eligible_candidates.len(),
                options.max_interaction_variables
            ));
        }
    }

    let diagnoses = if late_baseline_instability {
        Vec::new()
    } else {
        synthesize_diagnoses(&intervention_results, interaction_search.as_ref())
    };
    let any_changed = singles_changed
        || interaction_search
            .as_ref()
            .is_some_and(|result| result.changed);
    let failed_interventions = intervention_results
        .iter()
        .filter(|result| result.error.is_some())
        .count();

    let status = if late_baseline_instability {
        CheckStatus::UncontrolledNondeterminism
    } else {
        classify_status(
            any_changed,
            build_outcome_dependency,
            failed_interventions,
            interaction_failed,
        )
    };

    let mut notes = if late_baseline_instability {
        vec![
            "matched baseline trials performed for a stochastic intervention contradicted the initial control-stability gate".to_string(),
            "causal diagnoses and interaction minimization were suppressed because uncontrolled baseline nondeterminism was observed with additional repetition".to_string(),
        ]
    } else if any_changed {
        vec![
            "the controlled baseline was stable, but at least one tested environmental perturbation changed declared artifact bytes".to_string(),
            "diagnoses describe observed causal factors under the tested interventions, not universal root causes".to_string(),
        ]
    } else if build_outcome_dependency {
        vec![
            "the controlled baseline was stable, but at least one controlled input variant caused the build command to exit non-zero; this establishes build-outcome sensitivity under the tested intervention but does not by itself prove byte-level non-reproducibility".to_string(),
            "reproducibility is inconclusive because no comparable variant artifact was produced for at least one causal trial".to_string(),
        ]
    } else if status == CheckStatus::Inconclusive {
        vec!["the controlled baseline was stable, but at least one requested experiment could not be completed; reproducibility is therefore inconclusive".to_string()]
    } else {
        vec![format!(
            "all declared artifacts and build-success outcomes were stable across controls and all {} successful configured one-variable interventions",
            intervention_results.len().saturating_sub(failed_interventions)
        )]
    };

    if failed_interventions > 0 {
        notes.push(format!(
            "{failed_interventions} intervention(s) could not be completed and were excluded from positive reproducibility claims"
        ));
    }
    if let Some(note) = interaction_note {
        notes.push(note);
    }
    for result in &intervention_results {
        if let Some(statistical) = &result.stochastic_effect {
            notes.push(format!(
                "stochastic {} probe: baseline {}/{} changed, variant {}/{} changed, Fisher p={:.6} at alpha={:.3} ({:?})",
                result.intervention.variable,
                statistical.baseline_changed_trials,
                statistical.baseline_trials,
                statistical.variant_changed_trials,
                statistical.variant_trials,
                statistical.fisher_exact_p_value,
                statistical.alpha,
                statistical.classification
            ));
        }
    }
    if let Some(result) = &interaction_search {
        if result.changed && result.stable_effect {
            notes.push(format!(
                "ddmin found a {}-variable minimal observed interaction after {} subset test(s)",
                result.minimal_variables.len(),
                result.tested_subsets
            ));
        } else if !result.changed {
            notes.push(format!(
                "the combined deterministic intervention set did not change artifact bytes; no interaction was minimized across {} eligible variables",
                result.candidate_variables.len()
            ));
        } else if !result.stable_effect {
            notes.push("the combined intervention produced inconsistent outcomes and was not promoted to a causal interaction diagnosis".to_string());
        }
    }

    let report = CheckReport {
        schema_version: CHECK_REPORT_SCHEMA_CURRENT,
        experiment_id,
        status,
        source_digest,
        source_provenance,
        baseline_environment: baseline,
        runs,
        artifact_comparisons,
        interventions: intervention_results,
        interaction_search,
        diagnoses,
        notes,
    };
    persist_report(project_root, &report)?;
    Ok(report)
}

fn classify_status(
    artifact_effect_observed: bool,
    build_outcome_dependency: bool,
    failed_interventions: usize,
    interaction_failed: bool,
) -> CheckStatus {
    if artifact_effect_observed {
        CheckStatus::NonReproducible
    } else if build_outcome_dependency || failed_interventions > 0 || interaction_failed {
        CheckStatus::Inconclusive
    } else {
        CheckStatus::Reproducible
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_intervention<R: Runner>(
    project_root: &Path,
    runner: &R,
    spec: &BuildSpec,
    experiment_id: Uuid,
    source_digest: &str,
    baseline_run: &BuildRun,
    baseline_environment: &crate::model::ControlledEnvironment,
    planned: PlannedIntervention,
    options: &CheckOptions,
    ordinal: &mut usize,
) -> Result<InterventionResult> {
    let run_count = match planned.intervention.kind {
        InterventionKind::CpuCount => options.stochastic_runs.max(options.intervention_runs),
        InterventionKind::NetworkAccess
        | InterventionKind::ToolchainExecutable
        | InterventionKind::DependencyFile => options.intervention_runs.max(2),
        InterventionKind::BuildImage => options.intervention_runs,
        _ => options.intervention_runs,
    };
    let mut intervention_runs = Vec::with_capacity(run_count);
    let mut reference_runs = if planned.intervention.kind == InterventionKind::CpuCount {
        Vec::with_capacity(run_count)
    } else {
        Vec::new()
    };
    let mut build_failures = Vec::new();
    let mut error = None;

    if planned.intervention.kind == InterventionKind::CpuCount {
        // Interleave a baseline reference immediately before every CPU variant
        // trial. This limits simple temporal drift relative to collecting two
        // large blocks while preserving fixed, preconfigured sample sizes.
        for _ in 0..run_count {
            match runner.run(spec, experiment_id, source_digest, *ordinal, baseline_environment) {
                Ok(run) => {
                    persist_run(project_root, &run)?;
                    reference_runs.push(run);
                }
                Err(run_error) => {
                    error = Some(format!(
                        "matched stochastic baseline build failed: {run_error:#}"
                    ));
                    *ordinal += 1;
                    break;
                }
            }
            *ordinal += 1;

            match runner.run(spec, experiment_id, source_digest, *ordinal, &planned.environment) {
                Ok(run) => {
                    persist_run(project_root, &run)?;
                    intervention_runs.push(run);
                }
                Err(run_error) => {
                    error = Some(format!("stochastic intervention build failed: {run_error:#}"));
                    *ordinal += 1;
                    break;
                }
            }
            *ordinal += 1;
        }
    } else {
        for _ in 0..run_count {
            if build_failure_is_observable(planned.intervention.kind) {
                match runner.run_attempt(
                    spec,
                    experiment_id,
                    source_digest,
                    *ordinal,
                    &planned.environment,
                ) {
                    Ok(RunOutcome::Success(run)) => {
                        persist_run(project_root, &run)?;
                        intervention_runs.push(run);
                    }
                    Ok(RunOutcome::BuildFailed(failure)) => {
                        persist_build_failure(project_root, &failure)?;
                        build_failures.push(failure);
                    }
                    Err(run_error) => {
                        error = Some(format!("intervention infrastructure failed: {run_error:#}"));
                        *ordinal += 1;
                        break;
                    }
                }
            } else {
                match runner.run(spec, experiment_id, source_digest, *ordinal, &planned.environment) {
                    Ok(run) => {
                        persist_run(project_root, &run)?;
                        intervention_runs.push(run);
                    }
                    Err(run_error) => {
                        error = Some(format!("intervention build failed: {run_error:#}"));
                        *ordinal += 1;
                        break;
                    }
                }
            }
            *ordinal += 1;
        }
    }

    let artifact_deltas = if intervention_runs.is_empty() {
        Vec::new()
    } else {
        build_artifact_deltas(baseline_run, &intervention_runs, Some(&planned.intervention))
    };
    let changed = intervention_runs
        .iter()
        .any(|run| !same_artifact_hashes(baseline_run, run));
    let failure_observable = build_failure_is_observable(planned.intervention.kind);
    let build_failure_effect = failure_observable && !build_failures.is_empty();
    let distinct_variant_outcomes = if failure_observable {
        build_outcome_count(&intervention_runs, &build_failures)
    } else {
        distinct_artifact_outcomes(&intervention_runs)
    };
    let variant_stable = if failure_observable {
        (!(intervention_runs.is_empty() && build_failures.is_empty()))
            .then_some(build_outcomes_stable(&intervention_runs, &build_failures))
    } else {
        (!intervention_runs.is_empty()).then_some(distinct_variant_outcomes == 1)
    };
    let stochastic_effect = if planned.intervention.kind == InterventionKind::CpuCount
        && intervention_runs.len() == run_count
        && reference_runs.len() == run_count
    {
        let baseline_changed = reference_runs
            .iter()
            .filter(|run| !same_artifact_hashes(baseline_run, run))
            .count();
        let variant_changed = intervention_runs
            .iter()
            .filter(|run| !same_artifact_hashes(baseline_run, run))
            .count();
        Some(assess_stochastic_effect(
            baseline_changed,
            reference_runs.len(),
            variant_changed,
            intervention_runs.len(),
            options.stochastic_alpha,
        ))
    } else {
        None
    };

    let mut confirmation_run = None;
    let mut reverted_to_baseline = None;
    if (changed || build_failure_effect) && options.confirmation_runs == 1 {
        match runner.run(spec, experiment_id, source_digest, *ordinal, baseline_environment) {
            Ok(run) => {
                persist_run(project_root, &run)?;
                reverted_to_baseline = Some(same_artifact_hashes(baseline_run, &run));
                confirmation_run = Some(run);
            }
            Err(run_error) => {
                let message = format!("baseline reversion build failed: {run_error:#}");
                error = Some(match error {
                    Some(existing) => format!("{existing}; {message}"),
                    None => message,
                });
            }
        }
        *ordinal += 1;
    }

    Ok(InterventionResult {
        intervention: planned.intervention,
        runs: intervention_runs,
        reference_runs,
        build_failures,
        artifact_deltas,
        changed,
        variant_stable,
        distinct_variant_outcomes,
        stochastic_effect,
        reverted_to_baseline,
        confirmation_run,
        error,
    })
}

fn build_failure_is_observable(kind: InterventionKind) -> bool {
    matches!(
        kind,
        InterventionKind::NetworkAccess
            | InterventionKind::ToolchainExecutable
            | InterventionKind::DependencyFile
    )
}

fn build_outcomes_stable(runs: &[BuildRun], failures: &[crate::model::BuildFailure]) -> bool {
    if !runs.is_empty() && !failures.is_empty() {
        return false;
    }
    if !runs.is_empty() {
        return distinct_artifact_outcomes(runs) == 1;
    }
    let Some(first) = failures.first() else {
        return false;
    };
    failures.iter().skip(1).all(|failure| failure.exit_code == first.exit_code)
}

fn build_outcome_count(runs: &[BuildRun], failures: &[crate::model::BuildFailure]) -> usize {
    let mut count = distinct_artifact_outcomes(runs);
    let mut exit_codes = BTreeSet::new();
    for failure in failures {
        exit_codes.insert(failure.exit_code);
    }
    count += exit_codes.len();
    count
}

#[allow(clippy::too_many_arguments)]
fn execute_interaction_search<R: Runner>(
    project_root: &Path,
    runner: &R,
    spec: &BuildSpec,
    experiment_id: Uuid,
    source_digest: &str,
    baseline_run: &BuildRun,
    baseline_environment: &crate::model::ControlledEnvironment,
    candidates: &[PlannedIntervention],
    options: &CheckOptions,
    ordinal: &mut usize,
) -> Result<Option<InteractionSearchResult>> {
    let candidate_variables = candidates
        .iter()
        .map(|candidate| candidate.intervention.variable.clone())
        .collect::<Vec<_>>();

    let full_runs = run_interaction_subset(
        project_root,
        runner,
        spec,
        experiment_id,
        source_digest,
        baseline_environment,
        candidates,
        options.interaction_runs,
        ordinal,
    )?;
    let full_changed = full_runs
        .iter()
        .all(|run| !same_artifact_hashes(baseline_run, run));
    let full_stable = distinct_artifact_outcomes(&full_runs) == 1;

    if !(full_changed && full_stable) {
        let changed = full_runs
            .iter()
            .any(|run| !same_artifact_hashes(baseline_run, run));
        return Ok(Some(InteractionSearchResult {
            candidate_variables,
            minimal_variables: Vec::new(),
            tested_subsets: 1,
            artifact_deltas: build_artifact_deltas(baseline_run, &full_runs, None),
            runs: full_runs,
            changed,
            stable_effect: full_stable && full_changed,
            reverted_to_baseline: None,
            confirmation_run: None,
            error: None,
        }));
    }

    let ddmin_outcome = ddmin::minimize(candidates, |subset| {
        let subset_runs = run_interaction_subset(
            project_root,
            runner,
            spec,
            experiment_id,
            source_digest,
            baseline_environment,
            subset,
            options.interaction_runs,
            ordinal,
        )?;
        let changed = subset_runs
            .iter()
            .all(|run| !same_artifact_hashes(baseline_run, run));
        Ok(changed && distinct_artifact_outcomes(&subset_runs) == 1)
    })?;

    let minimal_variables = ddmin_outcome
        .minimal
        .iter()
        .map(|candidate| candidate.intervention.variable.clone())
        .collect::<Vec<_>>();
    let final_runs = run_interaction_subset(
        project_root,
        runner,
        spec,
        experiment_id,
        source_digest,
        baseline_environment,
        &ddmin_outcome.minimal,
        options.interaction_runs,
        ordinal,
    )?;
    let final_changed = final_runs
        .iter()
        .all(|run| !same_artifact_hashes(baseline_run, run));
    let final_stable = distinct_artifact_outcomes(&final_runs) == 1;
    let artifact_deltas = build_artifact_deltas(baseline_run, &final_runs, None);

    let mut confirmation_run = None;
    let mut reverted_to_baseline = None;
    if final_changed && final_stable && options.confirmation_runs == 1 {
        let run = runner.run(spec, experiment_id, source_digest, *ordinal, baseline_environment)?;
        persist_run(project_root, &run)?;
        reverted_to_baseline = Some(same_artifact_hashes(baseline_run, &run));
        confirmation_run = Some(run);
        *ordinal += 1;
    }

    Ok(Some(InteractionSearchResult {
        candidate_variables,
        minimal_variables,
        tested_subsets: 2 + ddmin_outcome.tests,
        runs: final_runs,
        artifact_deltas,
        changed: final_changed,
        stable_effect: final_changed && final_stable,
        reverted_to_baseline,
        confirmation_run,
        error: None,
    }))
}

#[allow(clippy::too_many_arguments)]
fn run_interaction_subset<R: Runner>(
    project_root: &Path,
    runner: &R,
    spec: &BuildSpec,
    experiment_id: Uuid,
    source_digest: &str,
    baseline_environment: &crate::model::ControlledEnvironment,
    subset: &[PlannedIntervention],
    run_count: usize,
    ordinal: &mut usize,
) -> Result<Vec<BuildRun>> {
    let environment = combine_interventions(baseline_environment, subset);
    let mut runs = Vec::with_capacity(run_count);
    for _ in 0..run_count {
        let run = runner.run(spec, experiment_id, source_digest, *ordinal, &environment)?;
        persist_run(project_root, &run)?;
        runs.push(run);
        *ordinal += 1;
    }
    Ok(runs)
}

pub(crate) fn same_artifact_hashes(left: &BuildRun, right: &BuildRun) -> bool {
    if left.artifacts.len() != right.artifacts.len() {
        return false;
    }
    left.artifacts.iter().all(|left_artifact| {
        right
            .artifacts
            .iter()
            .find(|right_artifact| right_artifact.logical_path == left_artifact.logical_path)
            .is_some_and(|right_artifact| right_artifact.sha256 == left_artifact.sha256)
    })
}

pub(crate) fn distinct_artifact_outcomes(runs: &[BuildRun]) -> usize {
    runs.iter().map(artifact_signature).collect::<BTreeSet<_>>().len()
}

pub(crate) fn artifact_signature(run: &BuildRun) -> Vec<(String, String)> {
    let mut signature = run
        .artifacts
        .iter()
        .map(|artifact| (
            artifact.logical_path.to_string_lossy().into_owned(),
            artifact.sha256.clone(),
        ))
        .collect::<Vec<_>>();
    signature.sort();
    signature
}

pub(crate) fn build_artifact_deltas(
    baseline: &BuildRun,
    all_variant_runs: &[BuildRun],
    intervention: Option<&Intervention>,
) -> Vec<ArtifactDelta> {
    baseline
        .artifacts
        .iter()
        .map(|baseline_artifact| {
            let variant_artifacts = all_variant_runs
                .iter()
                .filter_map(|run| {
                    run.artifacts
                        .iter()
                        .find(|artifact| artifact.logical_path == baseline_artifact.logical_path)
                })
                .collect::<Vec<_>>();
            let changed = variant_artifacts.len() != all_variant_runs.len()
                || variant_artifacts
                    .iter()
                    .any(|artifact| artifact.sha256 != baseline_artifact.sha256);
            let representative = variant_artifacts
                .iter()
                .copied()
                .find(|artifact| artifact.sha256 != baseline_artifact.sha256)
                .or_else(|| variant_artifacts.first().copied());

            let mut marker_evidence = Vec::new();
            let mut elf_debug_marker_evidence = Vec::new();
            let mut elf_marker_location_evidence = Vec::new();
            let mut archive_metadata_evidence = Vec::new();
            let mut semantic_evidence = Vec::new();
            let mut baseline_direct_marker = false;
            let mut variant_direct_marker = false;
            let mut direct_structural_evidence = false;

            if changed {
                if let Some(intervention) = intervention {
                    let baseline_markers = baseline_artifact
                        .embedded_markers
                        .iter()
                        .filter(|marker| marker_matches(intervention, marker))
                        .cloned()
                        .collect::<Vec<_>>();
                    let baseline_elf_markers = baseline_artifact
                        .elf_debug_markers
                        .iter()
                        .filter(|marker| marker_matches(intervention, marker))
                        .cloned()
                        .collect::<Vec<_>>();
                    let baseline_elf_locations = baseline_artifact
                        .elf_marker_locations
                        .iter()
                        .filter(|location| elf_location_matches(intervention, location))
                        .cloned()
                        .collect::<Vec<_>>();
                    baseline_direct_marker = !baseline_markers.is_empty()
                        || !baseline_elf_markers.is_empty()
                        || !baseline_elf_locations.is_empty();
                    marker_evidence.extend(baseline_markers);
                    elf_debug_marker_evidence.extend(baseline_elf_markers);
                    elf_marker_location_evidence.extend(baseline_elf_locations);

                    for run in all_variant_runs {
                        if let Some(artifact) = run
                            .artifacts
                            .iter()
                            .find(|artifact| artifact.logical_path == baseline_artifact.logical_path)
                        {
                            let variant_markers = artifact
                                .embedded_markers
                                .iter()
                                .filter(|marker| marker_matches(intervention, marker))
                                .cloned()
                                .collect::<Vec<_>>();
                            let variant_elf_markers = artifact
                                .elf_debug_markers
                                .iter()
                                .filter(|marker| marker_matches(intervention, marker))
                                .cloned()
                                .collect::<Vec<_>>();
                            let variant_elf_locations = artifact
                                .elf_marker_locations
                                .iter()
                                .filter(|location| elf_location_matches(intervention, location))
                                .cloned()
                                .collect::<Vec<_>>();
                            if !variant_markers.is_empty()
                                || !variant_elf_markers.is_empty()
                                || !variant_elf_locations.is_empty()
                            {
                                variant_direct_marker = true;
                            }
                            marker_evidence.extend(variant_markers);
                            elf_debug_marker_evidence.extend(variant_elf_markers);
                            elf_marker_location_evidence.extend(variant_elf_locations);
                        }
                    }
                }

                if let Some(variant_artifact) = representative {
                    if let (Some(baseline_metadata), Some(variant_metadata)) = (
                        baseline_artifact.archive_metadata.as_ref(),
                        variant_artifact.archive_metadata.as_ref(),
                    ) {
                        archive_metadata_evidence = compare_archive_metadata(baseline_metadata, variant_metadata);
                        if let Some(intervention) = intervention {
                            direct_structural_evidence = archive_evidence_matches_intervention(
                                intervention,
                                &archive_metadata_evidence,
                            );
                        }
                    }
                    if let (Some(baseline_semantic), Some(variant_semantic)) = (
                        baseline_artifact.semantic_metadata.as_ref(),
                        variant_artifact.semantic_metadata.as_ref(),
                    ) {
                        semantic_evidence = compare_semantic_metadata(baseline_semantic, variant_semantic);
                        if let Some(intervention) = intervention {
                            direct_structural_evidence |= semantic_evidence_matches_intervention(
                                intervention,
                                &semantic_evidence,
                            );
                        }
                    }
                }
            }

            sort_and_dedup_markers(&mut marker_evidence);
            sort_and_dedup_markers(&mut elf_debug_marker_evidence);
            sort_and_dedup_elf_locations(&mut elf_marker_location_evidence);

            ArtifactDelta {
                logical_path: baseline_artifact.logical_path.clone(),
                baseline_sha256: baseline_artifact.sha256.clone(),
                variant_sha256: representative
                    .map(|artifact| artifact.sha256.clone())
                    .unwrap_or_else(|| "<missing>".to_string()),
                changed,
                marker_evidence,
                elf_debug_marker_evidence,
                elf_marker_location_evidence,
                archive_metadata_evidence,
                semantic_evidence,
                baseline_direct_marker,
                variant_direct_marker,
                direct_structural_evidence,
            }
        })
        .collect()
}

fn archive_evidence_matches_intervention(
    intervention: &Intervention,
    evidence: &[ArchiveMetadataDifference],
) -> bool {
    evidence.iter().any(|difference| match intervention.kind {
        InterventionKind::SourceMtime | InterventionKind::SourceDateEpoch => matches!(
            difference.field,
            ArchiveMetadataField::Mtime | ArchiveMetadataField::ContainerMtime
        ),
        InterventionKind::Umask => difference.field == ArchiveMetadataField::Mode,
        InterventionKind::CpuCount | InterventionKind::DirectoryOrder => {
            difference.field == ArchiveMetadataField::MemberOrder
        }
        _ => false,
    })
}

fn semantic_evidence_matches_intervention(
    intervention: &Intervention,
    evidence: &[ArtifactSemanticDifference],
) -> bool {
    evidence.iter().any(|difference| {
        intervention.kind == InterventionKind::SourceDateEpoch
            && difference.field == "coff_timestamp"
    })
}

fn sort_and_dedup_markers(markers: &mut Vec<EmbeddedMarker>) {
    markers.sort_by(|left, right| {
        left.variable.cmp(&right.variable).then_with(|| left.value.cmp(&right.value))
    });
    markers.dedup();
}

fn sort_and_dedup_elf_locations(locations: &mut Vec<ElfMarkerLocation>) {
    locations.sort_by(|left, right| {
        left.variable
            .cmp(&right.variable)
            .then_with(|| left.value.cmp(&right.value))
            .then_with(|| left.section.cmp(&right.section))
    });
    locations.dedup();
}

fn elf_location_matches(intervention: &Intervention, location: &&ElfMarkerLocation) -> bool {
    marker_variable_matches(intervention, &location.variable)
}

fn marker_matches(intervention: &Intervention, marker: &&EmbeddedMarker) -> bool {
    marker_variable_matches(intervention, &marker.variable)
}

fn marker_variable_matches(intervention: &Intervention, variable: &str) -> bool {
    match intervention.kind {
        InterventionKind::SourcePath => variable == "source_path",
        InterventionKind::BuildPath => variable == "build_path",
        InterventionKind::SourceDateEpoch => variable == "SOURCE_DATE_EPOCH",
        InterventionKind::SourceMtime => false,
        InterventionKind::Timezone => variable == "TZ",
        InterventionKind::Locale => variable == "LANG" || variable == "LC_ALL",
        InterventionKind::Hostname => variable == "hostname",
        InterventionKind::EnvironmentVariable => variable == intervention.variable,
        InterventionKind::CpuCount => {
            variable == "cpu_count" || variable == "REPROBISECT_CPU_COUNT"
        }
        InterventionKind::BuildImage
        | InterventionKind::ToolchainExecutable
        | InterventionKind::DependencyFile
        | InterventionKind::NetworkAccess
        | InterventionKind::Umask
        | InterventionKind::DirectoryOrder => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_failure_without_variant_artifact_is_inconclusive() {
        assert_eq!(
            classify_status(false, true, 0, false),
            CheckStatus::Inconclusive
        );
    }

    #[test]
    fn narrow_input_build_failure_is_an_observable_but_not_a_byte_delta() {
        assert!(build_failure_is_observable(InterventionKind::ToolchainExecutable));
        assert!(build_failure_is_observable(InterventionKind::DependencyFile));
        assert_eq!(classify_status(false, true, 0, false), CheckStatus::Inconclusive);
    }

    #[test]
    fn observed_artifact_delta_is_non_reproducible_even_with_other_failures() {
        assert_eq!(
            classify_status(true, true, 1, true),
            CheckStatus::NonReproducible
        );
    }

    #[test]
    fn completed_stable_space_is_reproducible() {
        assert_eq!(
            classify_status(false, false, 0, false),
            CheckStatus::Reproducible
        );
    }
}
