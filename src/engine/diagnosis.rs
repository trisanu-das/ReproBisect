use std::path::PathBuf;

use crate::model::{
    ArchiveMetadataDifference, ArtifactSemanticDifference, Confidence, Diagnosis, EmbeddedMarker, InteractionSearchResult,
    InterventionKind, InterventionResult, StochasticEffectClassification,
};

pub fn synthesize_diagnoses(
    results: &[InterventionResult],
    interaction: Option<&InteractionSearchResult>,
) -> Vec<Diagnosis> {
    let mut diagnoses = results
        .iter()
        .filter(|result| observed_effect(result))
        .map(diagnosis_from_result)
        .collect::<Vec<_>>();

    if let Some(interaction) = interaction.filter(|result| {
        result.changed && result.stable_effect && result.minimal_variables.len() >= 2
    }) {
        diagnoses.push(diagnosis_from_interaction(interaction));
    }

    diagnoses
}

fn observed_effect(result: &InterventionResult) -> bool {
    if result.intervention.kind == InterventionKind::CpuCount {
        return result.changed
            && result.stochastic_effect.as_ref().is_some_and(|evidence| {
                evidence.classification == StochasticEffectClassification::Supported
            });
    }
    result.changed
        || (failure_observable(result.intervention.kind) && !result.build_failures.is_empty())
}

fn failure_observable(kind: InterventionKind) -> bool {
    matches!(
        kind,
        InterventionKind::NetworkAccess
            | InterventionKind::ToolchainExecutable
            | InterventionKind::DependencyFile
    )
}

fn diagnosis_from_result(result: &InterventionResult) -> Diagnosis {
    let changed_artifacts: Vec<PathBuf> = result
        .artifact_deltas
        .iter()
        .filter(|delta| delta.changed)
        .map(|delta| delta.logical_path.clone())
        .collect();

    let marker_evidence: Vec<EmbeddedMarker> = result
        .artifact_deltas
        .iter()
        .flat_map(|delta| delta.marker_evidence.iter().cloned())
        .collect();
    let elf_debug_marker_evidence: Vec<EmbeddedMarker> = result
        .artifact_deltas
        .iter()
        .flat_map(|delta| delta.elf_debug_marker_evidence.iter().cloned())
        .collect();

    let direct_marker_pair = result.artifact_deltas.iter().any(|delta| {
        delta.changed && delta.baseline_direct_marker && delta.variant_direct_marker
    });
    let direct_structural_evidence = result
        .artifact_deltas
        .iter()
        .any(|delta| delta.changed && delta.direct_structural_evidence);

    let variant_consistent = intervention_runs_consistent(result);
    let confidence = if result.error.is_some() {
        Confidence::Low
    } else if result.intervention.kind == InterventionKind::NetworkAccess {
        if result.variant_stable == Some(true) && result.reverted_to_baseline == Some(true) {
            Confidence::Medium
        } else {
            Confidence::Low
        }
    } else if result.intervention.kind == InterventionKind::BuildImage {
        // An image intervention bundles many toolchain/userspace variables; even
        // a perfectly repeatable effect cannot isolate a single compiler or linker.
        if result.reverted_to_baseline == Some(true) && variant_consistent {
            Confidence::Medium
        } else {
            Confidence::Low
        }
    } else if result.intervention.kind == InterventionKind::ToolchainExecutable {
        if result.reverted_to_baseline == Some(true)
            && variant_consistent
            && toolchain_binding_confirmed(result)
        {
            Confidence::High
        } else {
            // Without observed wrapper invocations, the experiment establishes
            // sensitivity to the binding value but not to execution of the selected tool.
            Confidence::Low
        }
    } else if result.intervention.kind == InterventionKind::DependencyFile {
        if result.reverted_to_baseline == Some(true)
            && variant_consistent
            && dependency_override_confirmed(result)
        {
            Confidence::High
        } else if result.reverted_to_baseline == Some(true) && variant_consistent {
            Confidence::Medium
        } else {
            Confidence::Low
        }
    } else if result.intervention.kind == InterventionKind::CpuCount
        && result.variant_stable == Some(false)
        && result.distinct_variant_outcomes >= 2
    {
        if result.reverted_to_baseline == Some(true)
            && result.stochastic_effect.as_ref().is_some_and(|evidence| {
                evidence.classification == StochasticEffectClassification::Supported
            })
        {
            Confidence::Medium
        } else {
            Confidence::Low
        }
    } else {
        match (
            result.reverted_to_baseline,
            direct_marker_pair || direct_structural_evidence,
            variant_consistent,
        ) {
            (Some(true), true, true) => Confidence::High,
            (Some(true), _, true) => Confidence::Medium,
            (_, true, true) => Confidence::Medium,
            _ => Confidence::Low,
        }
    };

    let mut evidence = if failure_observable(result.intervention.kind)
        && !result.build_failures.is_empty()
        && !result.changed
    {
        vec![format!(
            "changing {} from {:?} to {:?} caused the build command to exit non-zero in {} trial(s)",
            result.intervention.variable,
            result.intervention.baseline_value,
            result.intervention.variant_value,
            result.build_failures.len()
        )]
    } else {
        vec![format!(
            "changing {} from {:?} to {:?} changed {} declared artifact(s)",
            result.intervention.variable,
            result.intervention.baseline_value,
            result.intervention.variant_value,
            changed_artifacts.len()
        )]
    };

    if let Some(statistical) = &result.stochastic_effect {
        evidence.push(format!(
            "matched stochastic probe: baseline changed {}/{} ({:.3}), variant changed {}/{} ({:.3}), one-sided Fisher exact p={:.6} at alpha={:.3} ({:?})",
            statistical.baseline_changed_trials,
            statistical.baseline_trials,
            statistical.baseline_change_rate,
            statistical.variant_changed_trials,
            statistical.variant_trials,
            statistical.variant_change_rate,
            statistical.fisher_exact_p_value,
            statistical.alpha,
            statistical.classification
        ));
    }

    for failure in &result.build_failures {
        evidence.push(format!(
            "intervention build exited with code {} under controlled {:?} input",
            failure.exit_code, result.intervention.variable
        ));
    }

    if result.runs.len() > 1 {
        evidence.push(if variant_consistent {
            format!(
                "the intervention effect was byte-stable across {} intervention runs",
                result.runs.len()
            )
        } else {
            format!(
                "intervention runs produced {} distinct artifact outcome(s) across {} trial(s)",
                result.distinct_variant_outcomes,
                result.runs.len()
            )
        });
    }

    if let Some(error) = &result.error {
        evidence.push(format!("experiment warning: {error}"));
    }

    match result.reverted_to_baseline {
        Some(true) => evidence.push(
            "a subsequent baseline reversion reproduced the original baseline artifact bytes"
                .to_string(),
        ),
        Some(false) => evidence.push(
            "a subsequent baseline reversion did not reproduce the original baseline bytes"
                .to_string(),
        ),
        None => {}
    }

    if result.intervention.kind == InterventionKind::BuildImage {
        evidence.extend(toolchain_evidence(result));
    }

    if result.intervention.kind == InterventionKind::ToolchainExecutable {
        evidence.extend(toolchain_binding_evidence(result));
    }
    if result.intervention.kind == InterventionKind::DependencyFile {
        evidence.extend(dependency_override_evidence(result));
    }
    evidence.extend(network_trace_evidence(result));
    evidence.extend(process_trace_evidence(result));
    evidence.extend(runtime_dependency_evidence(result));

    for marker in &marker_evidence {
        evidence.push(format!(
            "artifact bytes contain a controlled value for {}: {:?}",
            marker.variable, marker.value
        ));
    }
    for marker in &elf_debug_marker_evidence {
        evidence.push(format!(
            "ELF debug bytes contain a controlled value for {}: {:?}",
            marker.variable, marker.value
        ));
    }
    for delta in result.artifact_deltas.iter().filter(|delta| delta.changed) {
        for location in &delta.elf_marker_location_evidence {
            evidence.push(format!(
                "ELF section {} contains controlled {} value {:?}",
                location.section, location.variable, location.value
            ));
        }
        for difference in &delta.archive_metadata_evidence {
            evidence.push(format_archive_difference(difference));
        }
        for difference in &delta.semantic_evidence {
            evidence.push(format_semantic_difference(difference));
        }
    }
    evidence.sort();
    evidence.dedup();

    let (title, remediation) = remediation_for(result);
    let mut limitations = vec![
        "causality is limited to the tested intervention space and build configuration".to_string(),
    ];
    if result.reverted_to_baseline != Some(true) {
        limitations.push(
            "the effect was not confirmed by a successful baseline reversion trial".to_string(),
        );
    }
    if !variant_consistent {
        limitations.push(
            "the intervention environment produced internally inconsistent artifact bytes"
                .to_string(),
        );
    }
    if result.intervention.kind == InterventionKind::DirectoryOrder {
        limitations.push(
            "directory-order perturbation changes source materialization order; actual readdir ordering remains filesystem-dependent"
                .to_string(),
        );
    }
    if result.intervention.kind == InterventionKind::BuildImage {
        limitations.push(
            "container-image interventions are intentionally coarse: compiler, linker, libc, package set, and other userspace inputs may change together"
                .to_string(),
        );
    }
    if result.intervention.kind == InterventionKind::ToolchainExecutable {
        limitations.push(
            "the executable binding is isolated within one resolved image, but the selected tools may invoke different subordinate programs or defaults; the claim is about the binding, not every internal compiler phase"
                .to_string(),
        );
        if !toolchain_binding_invoked_on_both_sides(result) {
            limitations.push(
                "the controlled tool executable was not observed through ReproBisect's invocation wrapper on both sides; this establishes sensitivity to the binding value, not confirmed execution of the selected tool"
                    .to_string(),
            );
        }
    }
    if result.intervention.kind == InterventionKind::DependencyFile {
        limitations.push(
            "the experiment establishes sensitivity to the controlled dependency declaration bytes; it does not prove which transitive package or resolver decision caused the artifact change"
                .to_string(),
        );
    }
    if result.intervention.kind == InterventionKind::NetworkAccess {
        limitations.push(
            "network disabling establishes sensitivity to network availability under this build flow; exact endpoints are intentionally redacted, and syscall tracing alone does not prove which downloaded bytes affected the build"
                .to_string(),
        );
    }

    if result.intervention.kind == InterventionKind::CpuCount {
        limitations.push(
            "the Fisher test compares a fixed finite set of matched baseline/variant trials and does not identify the internal race, schedule, or task that produced divergent bytes".to_string(),
        );
        limitations.push(
            "the configured alpha applies to this stochastic probe; ReproBisect does not currently correct across a user-defined family of many stochastic hypotheses".to_string(),
        );
    }

    Diagnosis {
        title,
        causal_variables: vec![result.intervention.variable.clone()],
        affected_artifacts: changed_artifacts,
        evidence,
        confidence,
        remediation,
        limitations,
    }
}

fn diagnosis_from_interaction(result: &InteractionSearchResult) -> Diagnosis {
    let affected_artifacts = result
        .artifact_deltas
        .iter()
        .filter(|delta| delta.changed)
        .map(|delta| delta.logical_path.clone())
        .collect::<Vec<_>>();

    let mut evidence = vec![format!(
        "no member of the minimized set was positive in the one-variable sweep, while the joint intervention on [{}] changed artifact bytes",
        result.minimal_variables.join(", ")
    )];
    evidence.push(format!(
        "ddmin required {} subset test(s) and returned a 1-minimal observed interaction of size {}",
        result.tested_subsets,
        result.minimal_variables.len()
    ));
    if result.reverted_to_baseline == Some(true) {
        evidence.push(
            "a subsequent baseline reversion reproduced the original baseline artifact bytes"
                .to_string(),
        );
    }

    Diagnosis {
        title: format!(
            "Build output depends on an interaction between {}",
            result.minimal_variables.join(" + ")
        ),
        causal_variables: result.minimal_variables.clone(),
        affected_artifacts,
        evidence,
        confidence: if result.reverted_to_baseline == Some(true) {
            Confidence::Medium
        } else {
            Confidence::Low
        },
        remediation: vec![
            "treat the listed variables as an interacting build-input set; normalize or eliminate the joint dependency, then repeat the original combined intervention"
                .to_string(),
        ],
        limitations: vec![
            "the set is 1-minimal only within the tested candidate variables and observed build configuration; it is not a proof of a globally unique root cause"
                .to_string(),
        ],
    }
}

fn intervention_runs_consistent(result: &InterventionResult) -> bool {
    if result.runs.len() <= 1 {
        return true;
    }
    if result
        .runs
        .iter()
        .any(|run| run.artifacts.len() != result.runs[0].artifacts.len())
    {
        return false;
    }

    (0..result.runs[0].artifacts.len()).all(|artifact_index| {
        let expected = &result.runs[0].artifacts[artifact_index].sha256;
        result
            .runs
            .iter()
            .skip(1)
            .all(|run| &run.artifacts[artifact_index].sha256 == expected)
    })
}

fn toolchain_binding_invoked_on_both_sides(result: &InterventionResult) -> bool {
    let variable = result
        .intervention
        .variable
        .strip_prefix("toolchain:")
        .unwrap_or(&result.intervention.variable);
    let variant_invoked = result.runs.iter().any(|run| {
        run.toolchain_provenance
            .bindings
            .iter()
            .any(|probe| probe.variable == variable && probe.invocation_count > 0)
    });
    let baseline_invoked = result.confirmation_run.as_ref().is_some_and(|run| {
        run.toolchain_provenance
            .bindings
            .iter()
            .any(|probe| probe.variable == variable && probe.invocation_count > 0)
    });
    baseline_invoked && variant_invoked
}

fn toolchain_binding_confirmed(result: &InterventionResult) -> bool {
    let variable = result
        .intervention
        .variable
        .strip_prefix("toolchain:")
        .unwrap_or(&result.intervention.variable);
    let variant = result.runs.iter().find_map(|run| {
        run.toolchain_provenance
            .bindings
            .iter()
            .find(|probe| probe.variable == variable && probe.resolved_path.is_some())
    });
    let baseline = result.confirmation_run.as_ref().and_then(|run| {
        run.toolchain_provenance
            .bindings
            .iter()
            .find(|probe| probe.variable == variable && probe.resolved_path.is_some())
    });
    matches!(
        (baseline, variant),
        (Some(left), Some(right))
            if left.invocation_count > 0
                && right.invocation_count > 0
                && (left.configured_value != right.configured_value
                    || left.resolved_path != right.resolved_path
                    || left.version != right.version)
    )
}

fn dependency_override_confirmed(result: &InterventionResult) -> bool {
    let Some(target) = result.intervention.variable.strip_prefix("dependency:") else {
        return false;
    };
    result.runs.iter().any(|run| {
        run.source_overrides.iter().any(|record| {
            record.target.to_string_lossy() == target
                && record.baseline_sha256 != record.variant_sha256
        })
    })
}

fn toolchain_binding_evidence(result: &InterventionResult) -> Vec<String> {
    let variable = result
        .intervention
        .variable
        .strip_prefix("toolchain:")
        .unwrap_or(&result.intervention.variable);
    let mut evidence = Vec::new();
    if let Some(baseline) = result.confirmation_run.as_ref().and_then(|run| {
        run.toolchain_provenance
            .bindings
            .iter()
            .find(|probe| probe.variable == variable)
    }) {
        evidence.push(format!(
            "baseline {variable} binding resolved as configured={:?}, path={:?}, version={:?}, observed_invocations={}",
            baseline.configured_value, baseline.resolved_path, baseline.version, baseline.invocation_count
        ));
    }
    for probe in result.runs.iter().filter_map(|run| {
        run.toolchain_provenance
            .bindings
            .iter()
            .find(|probe| probe.variable == variable)
    }) {
        evidence.push(format!(
            "variant {variable} binding resolved as configured={:?}, path={:?}, version={:?}, observed_invocations={}",
            probe.configured_value, probe.resolved_path, probe.version, probe.invocation_count
        ));
        if let Some(error) = &probe.error {
            evidence.push(format!("variant {variable} probe limitation: {error}"));
        }
    }
    evidence.sort();
    evidence.dedup();
    evidence
}

fn dependency_override_evidence(result: &InterventionResult) -> Vec<String> {
    let mut evidence = Vec::new();
    for record in result.runs.iter().flat_map(|run| run.source_overrides.iter()) {
        evidence.push(format!(
            "fresh-workspace dependency override {} <- {} changed declaration sha256 {} -> {}",
            record.target.display(),
            record.variant_source.display(),
            record.baseline_sha256,
            record.variant_sha256
        ));
    }
    for record in result
        .build_failures
        .iter()
        .flat_map(|failure| failure.source_overrides.iter())
    {
        evidence.push(format!(
            "failed variant used dependency override {} <- {} with sha256 {} -> {}",
            record.target.display(),
            record.variant_source.display(),
            record.baseline_sha256,
            record.variant_sha256
        ));
    }
    evidence.sort();
    evidence.dedup();
    evidence
}

fn network_trace_evidence(result: &InterventionResult) -> Vec<String> {
    let mut evidence = Vec::new();
    let summaries = result
        .runs
        .iter()
        .map(|run| &run.network_trace)
        .chain(result.build_failures.iter().map(|failure| &failure.network_trace))
        .chain(result.confirmation_run.iter().map(|run| &run.network_trace));
    for summary in summaries.filter(|summary| summary.attempted) {
        if summary.tracer_available {
            let scopes = summary
                .endpoint_scopes
                .iter()
                .map(|(scope, count)| format!("{scope}:{count}"))
                .collect::<Vec<_>>()
                .join(",");
            let successful_scopes = summary
                .successful_endpoint_scopes
                .iter()
                .map(|(scope, count)| format!("{scope}:{count}"))
                .collect::<Vec<_>>()
                .join(",");
            evidence.push(format!(
                "redacted network syscall trace observed socket={} connect={} (success={} failed={}) sendto={} recvfrom={} families=[{}] scopes=[{}] successful_scopes=[{}] parsed_bytes={} truncated={}",
                summary.socket_calls,
                summary.connect_calls,
                summary.successful_connects,
                summary.failed_connects,
                summary.sendto_calls,
                summary.recvfrom_calls,
                summary.address_families.join(","),
                scopes,
                successful_scopes,
                summary.parsed_bytes,
                summary.trace_truncated,
            ));
        } else if let Some(error) = &summary.error {
            evidence.push(format!("network trace limitation: {error}"));
        }
    }
    evidence.sort();
    evidence.dedup();
    evidence
}

fn process_trace_evidence(result: &InterventionResult) -> Vec<String> {
    let mut evidence = Vec::new();
    let summaries = result
        .runs
        .iter()
        .map(|run| &run.process_trace)
        .chain(result.build_failures.iter().map(|failure| &failure.process_trace))
        .chain(result.confirmation_run.iter().map(|run| &run.process_trace));
    for summary in summaries.filter(|summary| summary.attempted) {
        if summary.tracer_available {
            let dependency_reads: usize = summary.processes.iter().map(|item| item.dependency_reads).sum();
            let cache_reads: usize = summary.processes.iter().map(|item| item.cache_reads).sum();
            let output_writes: usize = summary.processes.iter().map(|item| item.output_writes).sum();
            let nonlocal: usize = summary
                .processes
                .iter()
                .map(|item| item.successful_nonlocal_network_events)
                .sum();
            evidence.push(format!(
                "redacted process/file trace: processes={} dependency_reads={} cache_reads={} output_writes={} successful_nonlocal_events={} same_process(dependency+output={} cache+output={} network+cache={} network+cache+output={}) parsed_bytes={} truncated={} (same-process co-occurrence is not byte-level taint tracking)",
                summary.processes.len(),
                dependency_reads,
                cache_reads,
                output_writes,
                nonlocal,
                summary.same_process_dependency_output,
                summary.same_process_cache_output,
                summary.same_process_network_cache,
                summary.same_process_network_cache_output,
                summary.parsed_bytes,
                summary.truncated,
            ));
        } else if let Some(error) = &summary.error {
            evidence.push(format!("process/file trace limitation: {error}"));
        }
    }
    evidence.sort();
    evidence.dedup();
    evidence
}

fn runtime_dependency_evidence(result: &InterventionResult) -> Vec<String> {
    let mut evidence = Vec::new();
    let summaries = result
        .runs
        .iter()
        .map(|run| &run.runtime_dependency_provenance)
        .chain(
            result
                .build_failures
                .iter()
                .map(|failure| &failure.runtime_dependency_provenance),
        )
        .chain(
            result
                .confirmation_run
                .iter()
                .map(|run| &run.runtime_dependency_provenance),
        );
    for summary in summaries.filter(|summary| summary.attempted) {
        for resolution in &summary.dependency_resolutions {
            evidence.push(format!(
                "post-build {} resolution {}: parsed={} packages={} normalized_sha256={}",
                resolution.ecosystem,
                resolution.path.display(),
                resolution.parsed,
                resolution.package_count,
                resolution.normalized_sha256.as_deref().unwrap_or("<unavailable>")
            ));
        }
        for cache in &summary.cache_summaries {
            evidence.push(format!(
                "redacted {} dependency-cache content: before(files={} bytes={} aggregate_sha256={} truncated={}) after(files={} bytes={} aggregate_sha256={} truncated={}) complete={} changed={} observed_change={} bounds(files={} bytes={})",
                cache.ecosystem,
                cache.before_file_count,
                cache.before_total_bytes,
                cache.before_aggregate_sha256.as_deref().unwrap_or("<unavailable>"),
                cache.before_truncated,
                cache.after_file_count,
                cache.after_total_bytes,
                cache.after_aggregate_sha256.as_deref().unwrap_or("<unavailable>"),
                cache.after_truncated,
                cache.comparison_complete,
                cache.changed_during_build,
                cache.observed_change,
                cache.max_files,
                cache.max_bytes,
            ));
        }
        let correlation = &summary.network_cache_correlation;
        if correlation.attempted {
            evidence.push(format!(
                "network/cache build-window co-occurrence: trace_available={} successful_nonlocal_events={} complete_cache_mutations={} incomplete_cache_observations={} cooccurrence={} (co-occurrence does not identify byte origin)",
                correlation.network_trace_available,
                correlation.successful_nonlocal_network_events,
                correlation.complete_cache_mutations,
                correlation.incomplete_cache_observations,
                correlation.build_window_cooccurrence,
            ));
        }
        if let Some(error) = &summary.error {
            evidence.push(format!("runtime dependency provenance limitation: {error}"));
        }
    }
    evidence.sort();
    evidence.dedup();
    evidence
}

fn toolchain_evidence(result: &InterventionResult) -> Vec<String> {
    use std::collections::BTreeMap;

    let Some(variant) = result.runs.first() else {
        return Vec::new();
    };
    let mut evidence = vec![format!(
        "variant image {:?} resolved to {}",
        variant.image, variant.resolved_image_id
    )];

    if let Some(baseline) = result.confirmation_run.as_ref() {
        evidence.push(format!(
            "baseline image {:?} resolved to {}",
            baseline.image, baseline.resolved_image_id
        ));
        let left = baseline
            .toolchain_provenance
            .probes
            .iter()
            .map(|probe| (probe.tool.as_str(), probe.version.as_str()))
            .collect::<BTreeMap<_, _>>();
        let right = variant
            .toolchain_provenance
            .probes
            .iter()
            .map(|probe| (probe.tool.as_str(), probe.version.as_str()))
            .collect::<BTreeMap<_, _>>();
        for tool in left.keys().chain(right.keys()).copied().collect::<std::collections::BTreeSet<_>>() {
            if left.get(tool) != right.get(tool) {
                evidence.push(format!(
                    "toolchain probe {tool}: {:?} -> {:?}",
                    left.get(tool),
                    right.get(tool)
                ));
            }
        }
    } else {
        for probe in &variant.toolchain_provenance.probes {
            evidence.push(format!("variant toolchain {}: {}", probe.tool, probe.version));
        }
    }

    if let Some(error) = &variant.toolchain_provenance.error {
        evidence.push(format!("variant toolchain probe limitation: {error}"));
    }
    evidence
}

fn format_archive_difference(difference: &ArchiveMetadataDifference) -> String {
    let member = difference
        .member
        .as_deref()
        .map(|member| format!(" for member {member:?}"))
        .unwrap_or_default();
    format!(
        "archive metadata {:?}{} changed from {:?} to {:?}",
        difference.field, member, difference.baseline, difference.variant
    )
}

fn format_semantic_difference(difference: &ArtifactSemanticDifference) -> String {
    format!(
        "artifact semantic field {} changed from {:?} to {:?}",
        difference.field, difference.baseline, difference.variant
    )
}

fn has_semantic_field(result: &InterventionResult, field: &str) -> bool {
    result
        .artifact_deltas
        .iter()
        .flat_map(|delta| delta.semantic_evidence.iter())
        .any(|difference| difference.field == field)
}

fn remediation_for(result: &InterventionResult) -> (String, Vec<String>) {
    match result.intervention.kind {
        InterventionKind::BuildImage => (
            "Build output depends on the container/toolchain image".to_string(),
            vec![
                "pin the build image by immutable digest and record compiler/linker/package versions as declared build inputs".to_string(),
                "narrow the image-level effect with a smaller toolchain-only intervention before attributing the cause to a specific compiler or linker".to_string(),
            ],
        ),
        InterventionKind::ToolchainExecutable => (
            if toolchain_binding_invoked_on_both_sides(result) {
                if result.build_failures.is_empty() {
                    format!("Build output depends on {} executable selection", result.intervention.variable)
                } else {
                    format!("Build success depends on {} executable selection", result.intervention.variable)
                }
            } else if result.build_failures.is_empty() {
                format!("Build output depends on {} binding value", result.intervention.variable)
            } else {
                format!("Build success depends on {} binding value", result.intervention.variable)
            },
            vec![
                "pin the selected compiler/linker/archive executable and version as an explicit build input".to_string(),
                "if portability across tool versions is required, compare emitted metadata/flags and narrow the difference to a compiler phase or linker option".to_string(),
            ],
        ),
        InterventionKind::DependencyFile => (
            if result.build_failures.is_empty() {
                format!("Build output depends on {}", result.intervention.variable)
            } else {
                format!("Build success depends on {}", result.intervention.variable)
            },
            vec![
                "pin the dependency declaration/lockfile and verify resolved package content by digest in offline builds".to_string(),
                "narrow multi-package lockfile changes to one dependency coordinate before attributing the effect to a specific package".to_string(),
            ],
        ),
        InterventionKind::NetworkAccess => (
            if result.build_failures.is_empty() {
                "Build output depends on network availability".to_string()
            } else {
                "Build requires network availability".to_string()
            },
            vec![
                "vendor or prefetch dependencies into a content-addressed cache and run the build with networking disabled".to_string(),
                "pin remote inputs by cryptographic digest and treat any unavoidable network fetches as explicit declarative inputs".to_string(),
            ],
        ),
        InterventionKind::SourcePath => (
            "Build output depends on the source path".to_string(),
            vec![
                "avoid embedding absolute source paths in generated outputs".to_string(),
                "for GCC/Clang, consider -ffile-prefix-map=<source>=. and -fdebug-prefix-map=<source>=.; for rustc, consider --remap-path-prefix=<source>=."
                    .to_string(),
            ],
        ),
        InterventionKind::BuildPath => (
            "Build output depends on the build/workspace path".to_string(),
            vec![
                "avoid embedding absolute build paths in generated outputs".to_string(),
                "for GCC/Clang, consider -ffile-prefix-map=<build>=. and -fdebug-prefix-map=<build>=.; for rustc, consider --remap-path-prefix=<build>=."
                    .to_string(),
            ],
        ),
        InterventionKind::SourceDateEpoch => {
            let mut remediations = vec![
                "make generated timestamps deterministic and honor SOURCE_DATE_EPOCH where applicable"
                    .to_string(),
                "avoid embedding wall-clock build time unless it is an intentional declarative input"
                    .to_string(),
            ];
            if has_semantic_field(result, "coff_timestamp") {
                remediations.push(
                    "for PE/COFF outputs, enable the selected linker/toolchain's deterministic timestamp or reproducible-build mode rather than emitting a wall-clock COFF timestamp"
                        .to_string(),
                );
            }
            (
                "Build output depends on build timestamp input".to_string(),
                remediations,
            )
        }
        InterventionKind::SourceMtime => (
            "Build output depends on source filesystem mtimes".to_string(),
            vec![
                "normalize archive/member timestamps from SOURCE_DATE_EPOCH or another declared epoch"
                    .to_string(),
                "do not copy source mtimes into output metadata unless they are intentional build inputs"
                    .to_string(),
            ],
        ),
        InterventionKind::Timezone => (
            "Build output depends on timezone".to_string(),
            vec![
                "run time-sensitive generation with a fixed timezone such as TZ=UTC".to_string(),
                "serialize timestamps in an explicit timezone rather than the host default".to_string(),
            ],
        ),
        InterventionKind::Locale => (
            "Build output depends on locale".to_string(),
            vec![
                "pin LANG/LC_ALL for build-time sorting, formatting, parsing, and generated text"
                    .to_string(),
                "use locale-independent ordering and formatting where output bytes are expected to be reproducible"
                    .to_string(),
            ],
        ),
        InterventionKind::Hostname => (
            "Build output depends on hostname".to_string(),
            vec![
                "avoid embedding the build host name in artifacts or generated metadata".to_string(),
                "replace host-derived identifiers with stable declarative build metadata".to_string(),
            ],
        ),
        InterventionKind::EnvironmentVariable => (
            format!(
                "Build output depends on environment variable {}",
                result.intervention.variable
            ),
            vec![format!(
                "remove, normalize, or explicitly declare {} as a build input",
                result.intervention.variable
            )],
        ),
        InterventionKind::CpuCount => (
            if result.variant_stable == Some(false) {
                "Parallel build exhibits nondeterministic output".to_string()
            } else {
                "Build output depends on CPU/parallelism setting".to_string()
            },
            vec![
                "make aggregation, link, and archive ordering independent of task completion order"
                    .to_string(),
                "investigate race conditions and non-deterministic parallel build steps before merely forcing serial execution"
                    .to_string(),
            ],
        ),
        InterventionKind::Umask => (
            "Build output depends on process umask".to_string(),
            vec![
                "normalize output permissions explicitly rather than inheriting the ambient process umask"
                    .to_string(),
                "when creating archives, write deterministic member modes".to_string(),
            ],
        ),
        InterventionKind::DirectoryOrder => (
            "Build output is sensitive to source materialization/enumeration order".to_string(),
            vec![
                "sort filesystem enumeration results before aggregation, archiving, linking, or code generation"
                    .to_string(),
                "make member ordering explicit instead of relying on readdir/find/glob traversal order"
                    .to_string(),
            ],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArtifactDelta, Intervention};

    #[test]
    fn interaction_diagnosis_is_medium_only_after_reversion() {
        let result = InteractionSearchResult {
            candidate_variables: vec!["A".into(), "B".into()],
            minimal_variables: vec!["A".into(), "B".into()],
            tested_subsets: 4,
            runs: vec![],
            artifact_deltas: vec![ArtifactDelta {
                logical_path: "out".into(),
                baseline_sha256: "a".into(),
                variant_sha256: "b".into(),
                changed: true,
                marker_evidence: vec![],
                elf_debug_marker_evidence: vec![],
                elf_marker_location_evidence: vec![],
                archive_metadata_evidence: vec![],
                semantic_evidence: vec![],
                baseline_direct_marker: false,
                variant_direct_marker: false,
                direct_structural_evidence: false,
            }],
            changed: true,
            stable_effect: true,
            reverted_to_baseline: Some(true),
            confirmation_run: None,
            error: None,
        };
        let diagnosis = diagnosis_from_interaction(&result);
        assert_eq!(diagnosis.confidence, Confidence::Medium);
    }

    #[test]
    fn remediation_covers_source_path() {
        let result = InterventionResult {
            intervention: Intervention {
                id: "source-path".into(),
                kind: InterventionKind::SourcePath,
                variable: "source_path".into(),
                baseline_value: "/src".into(),
                variant_value: "/alt".into(),
                description: String::new(),
            },
            runs: vec![],
            reference_runs: vec![],
            build_failures: vec![],
            artifact_deltas: vec![],
            changed: true,
            variant_stable: Some(true),
            distinct_variant_outcomes: 1,
            stochastic_effect: None,
            reverted_to_baseline: Some(true),
            confirmation_run: None,
            error: None,
        };
        let (title, _) = remediation_for(&result);
        assert!(title.contains("source path"));
    }
}
