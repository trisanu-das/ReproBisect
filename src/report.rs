use anyhow::Result;

use crate::model::{
    CheckReport, CheckStatus, Confidence, EnvironmentComparisonReport, EnvironmentComparisonStatus,
};

pub fn print_text(report: &CheckReport, verbose: bool) {
    print_diagnosis_summary(report);
    if !verbose {
        return;
    }

    println!();
    println!("Detailed experiment evidence");
    println!("experiment: {}", report.experiment_id);
    println!("source sha256: {}", report.source_digest);
    if let Some(commit) = &report.source_provenance.git_commit {
        let dirty = match report.source_provenance.git_dirty {
            Some(true) => "dirty",
            Some(false) => "clean",
            None => "unknown",
        };
        println!("git: {} ({dirty})", commit);
    }
    if !report.source_provenance.dependency_files.is_empty() {
        println!(
            "dependency fingerprints: {} file(s)",
            report.source_provenance.dependency_files.len()
        );
        if verbose {
            for dependency in &report.source_provenance.dependency_files {
                println!(
                    "  {}  sha256={}  size={}",
                    dependency.path.display(),
                    dependency.sha256,
                    dependency.size_bytes
                );
            }
        }
    }
    if !report.source_provenance.dependency_resolutions.is_empty() {
        println!(
            "dependency resolution summaries: {}",
            report.source_provenance.dependency_resolutions.len()
        );
        for resolution in &report.source_provenance.dependency_resolutions {
            println!(
                "  {}  ecosystem={} parsed={} packages={} normalized_sha256={}",
                resolution.path.display(),
                resolution.ecosystem,
                resolution.parsed,
                resolution.package_count,
                resolution
                    .normalized_sha256
                    .as_deref()
                    .map(short_hash)
                    .unwrap_or("<unavailable>")
            );
            if verbose {
                if let Some(error) = &resolution.error {
                    println!("    parser note: {error}");
                }
            }
        }
    }
    println!("control runs: {}", report.runs.len());
    println!(
        "baseline: source_path={} build_path={} copy_order={:?} TZ={} locale={} hostname={}",
        report.baseline_environment.container_source_path,
        report.baseline_environment.container_work_path,
        report.baseline_environment.source_copy_order,
        report
            .baseline_environment
            .environment
            .get("TZ")
            .map(String::as_str)
            .unwrap_or("<unset>"),
        report
            .baseline_environment
            .environment
            .get("LC_ALL")
            .map(String::as_str)
            .unwrap_or("<unset>"),
        report
            .baseline_environment
            .hostname
            .as_deref()
            .unwrap_or("<default>")
    );
    println!(
        "baseline controls: network={} network_trace={} file_input_trace={} trace_parse_limit={} runtime_dependency_provenance={} image_override={} toolchain_bindings={} dependency_overrides={}",
        report.baseline_environment.network_mode,
        report.baseline_environment.network_trace,
        report.baseline_environment.file_input_trace,
        report.baseline_environment.syscall_trace_max_bytes,
        report.baseline_environment.runtime_dependency_provenance,
        report
            .baseline_environment
            .image_override
            .as_deref()
            .unwrap_or("<none>"),
        report.baseline_environment.toolchain_bindings.len(),
        report.baseline_environment.source_file_overrides.len()
    );
    println!();

    for run in &report.runs {
        println!(
            "control run {}  id={}  duration={}ms  runner={:?}  image={} ({})",
            run.ordinal,
            run.run_id,
            run.duration_ms,
            run.runner_backend,
            run.image,
            short_hash(&run.resolved_image_id)
        );
        if verbose {
            print_toolchain_provenance(&run.toolchain_provenance);
            print_source_overrides(&run.source_overrides);
            print_network_trace(&run.network_trace);
            print_process_trace(&run.process_trace);
            print_runtime_dependency_provenance(&run.runtime_dependency_provenance);
        }
        for artifact in &run.artifacts {
            println!(
                "  {}  sha256={}  size={}  type={:?}",
                artifact.logical_path.display(),
                artifact.sha256,
                artifact.size_bytes,
                artifact.detected_type
            );
        }

        if verbose {
            print_run_logs(run);
        }
    }

    println!();
    for comparison in &report.artifact_comparisons {
        println!(
            "control artifact {}: {}",
            comparison.logical_path.display(),
            if comparison.equal_across_runs {
                "STABLE"
            } else {
                "UNSTABLE"
            }
        );
    }

    if !report.interventions.is_empty() {
        println!();
        println!("interventions:");
        for result in &report.interventions {
            let outcome = if result.error.is_some() && result.runs.is_empty() && result.build_failures.is_empty() {
                "ERROR"
            } else if !result.build_failures.is_empty() && !result.runs.is_empty() {
                "MIXED"
            } else if !result.build_failures.is_empty() {
                "BUILD FAILED"
            } else if result.changed {
                "CHANGED"
            } else {
                "NO EFFECT"
            };
            println!(
                "  {:<24} {}  {:?} -> {:?}",
                result.intervention.variable,
                outcome,
                result.intervention.baseline_value,
                result.intervention.variant_value
            );
            if result.changed {
                for delta in result.artifact_deltas.iter().filter(|delta| delta.changed) {
                    println!(
                        "    {}  {} -> {}",
                        delta.logical_path.display(),
                        short_hash(&delta.baseline_sha256),
                        short_hash(&delta.variant_sha256)
                    );
                }
            }
            if let Some(statistical) = &result.stochastic_effect {
                println!(
                    "    stochastic matched trials: baseline {}/{} changed, variant {}/{} changed, p={:.6}, alpha={:.3}, classification={:?}",
                    statistical.baseline_changed_trials,
                    statistical.baseline_trials,
                    statistical.variant_changed_trials,
                    statistical.variant_trials,
                    statistical.fisher_exact_p_value,
                    statistical.alpha,
                    statistical.classification
                );
            }
            if !result.build_failures.is_empty() {
                let codes = result
                    .build_failures
                    .iter()
                    .map(|failure| failure.exit_code.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!(
                    "    build-command failures: {} trial(s), exit code(s): {}",
                    result.build_failures.len(),
                    codes
                );
            }
            if (result.changed || !result.build_failures.is_empty())
                && result.reverted_to_baseline.is_some()
            {
                println!(
                    "    baseline reversion: {}",
                    if result.reverted_to_baseline == Some(true) {
                        "CONFIRMED"
                    } else {
                        "FAILED"
                    }
                );
            }
            if let Some(error) = &result.error {
                println!("    warning: {error}");
            }
            if verbose {
                for run in &result.runs {
                    print_run_logs(run);
                    print_toolchain_provenance(&run.toolchain_provenance);
                    print_source_overrides(&run.source_overrides);
                    print_network_trace(&run.network_trace);
                    print_process_trace(&run.process_trace);
                    print_runtime_dependency_provenance(&run.runtime_dependency_provenance);
                }
                for run in &result.reference_runs {
                    println!("    matched stochastic baseline run {}:", run.ordinal);
                    print_run_logs(run);
                }
                for failure in &result.build_failures {
                    print_failure_logs(failure);
                    print_toolchain_provenance(&failure.toolchain_provenance);
                    print_source_overrides(&failure.source_overrides);
                    print_network_trace(&failure.network_trace);
                    print_process_trace(&failure.process_trace);
                    print_runtime_dependency_provenance(&failure.runtime_dependency_provenance);
                }
                if let Some(run) = &result.confirmation_run {
                    print_run_logs(run);
                }
            }
        }
    }

    if !report.diagnoses.is_empty() {
        println!();
        println!("diagnoses:");
        for (index, diagnosis) in report.diagnoses.iter().enumerate() {
            println!(
                "  {}. {}  [confidence: {}]",
                index + 1,
                diagnosis.title,
                confidence_label(&diagnosis.confidence)
            );
            println!(
                "     causal variable(s): {}",
                diagnosis.causal_variables.join(", ")
            );
            for evidence in &diagnosis.evidence {
                println!("     evidence: {evidence}");
            }
            for remediation in &diagnosis.remediation {
                println!("     remediation: {remediation}");
            }
        }
    }

    println!();
    match report.status {
        CheckStatus::Reproducible => println!("status: REPRODUCIBLE_WITHIN_TESTED_SPACE"),
        CheckStatus::NonReproducible => println!("status: NON_REPRODUCIBLE"),
        CheckStatus::UncontrolledNondeterminism => {
            println!("status: UNCONTROLLED_NONDETERMINISM")
        }
        CheckStatus::Inconclusive => println!("status: INCONCLUSIVE"),
    }
    for note in &report.notes {
        println!("note: {note}");
    }
}


fn print_diagnosis_summary(report: &CheckReport) {
    println!("ReproBisect diagnosis");
    println!("result: {}", check_status_label(&report.status));

    let controls_stable = !report.artifact_comparisons.is_empty()
        && report
            .artifact_comparisons
            .iter()
            .all(|comparison| comparison.equal_across_runs);
    println!(
        "controls: {} run(s), {}",
        report.runs.len(),
        if controls_stable { "stable" } else { "not stable" }
    );

    if report.diagnoses.is_empty() {
        match report.status {
            CheckStatus::Reproducible => {
                println!("cause: none found within the tested intervention space");
            }
            CheckStatus::UncontrolledNondeterminism => {
                println!("cause: not attributed; identical controlled baseline runs changed");
            }
            CheckStatus::Inconclusive => {
                println!("cause: inconclusive under the completed experiments");
            }
            CheckStatus::NonReproducible => {
                println!("cause: artifact difference observed, but no diagnosis was promoted");
            }
        }
    } else {
        for (index, diagnosis) in report.diagnoses.iter().enumerate() {
            println!();
            if report.diagnoses.len() == 1 {
                println!("cause: {}", diagnosis.title);
            } else {
                println!("cause {}: {}", index + 1, diagnosis.title);
            }
            println!(
                "causal variable{}: {}",
                if diagnosis.causal_variables.len() == 1 { "" } else { "s" },
                diagnosis.causal_variables.join(", ")
            );
            if !diagnosis.affected_artifacts.is_empty() {
                println!(
                    "artifact{}: {}",
                    if diagnosis.affected_artifacts.len() == 1 { "" } else { "s" },
                    diagnosis
                        .affected_artifacts
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            println!("confidence: {}", confidence_label(&diagnosis.confidence));

            for artifact in &diagnosis.affected_artifacts {
                if let Some((baseline, variant, reversion)) =
                    diagnosed_artifact_hashes(report, &diagnosis.causal_variables, artifact)
                {
                    println!("hashes for {}:", artifact.display());
                    println!("  baseline:  {}", short_hash(baseline));
                    println!("  variant:   {}", short_hash(variant));
                    match reversion {
                        Some((hash, true)) => {
                            println!("  reversion: {} (confirmed)", short_hash(hash))
                        }
                        Some((hash, false)) => {
                            println!("  reversion: {} (did not recover baseline)", short_hash(hash))
                        }
                        None => println!("  reversion: <not recorded>"),
                    }
                }
            }

            if !diagnosis.evidence.is_empty() {
                println!("evidence:");
                for evidence in &diagnosis.evidence {
                    println!("  - {evidence}");
                }
            }
            if !diagnosis.remediation.is_empty() {
                println!("likely remediation:");
                for remediation in &diagnosis.remediation {
                    println!("  - {remediation}");
                }
            }
            if !diagnosis.limitations.is_empty() {
                println!("limitations:");
                for limitation in &diagnosis.limitations {
                    println!("  - {limitation}");
                }
            }
        }
    }

    println!();
    println!("tested variables:");
    if report.interventions.is_empty() {
        println!("  (none completed)");
    } else {
        for result in &report.interventions {
            println!(
                "  {:<24} {}{}",
                result.intervention.variable,
                intervention_summary_label(result),
                reversion_summary_suffix(result)
            );
        }
    }

    if let Some(interaction) = &report.interaction_search {
        if !interaction.candidate_variables.is_empty() {
            let outcome = if interaction.changed && interaction.stable_effect {
                "EFFECT"
            } else if interaction.error.is_some() {
                "ERROR"
            } else {
                "no promoted effect"
            };
            println!(
                "  interaction({}) {}{}",
                interaction.candidate_variables.join(", "),
                outcome,
                match interaction.reverted_to_baseline {
                    Some(true) => " [reversion confirmed]",
                    Some(false) => " [reversion failed]",
                    None => "",
                }
            );
        }
    }

    for note in &report.notes {
        println!("note: {note}");
    }
    println!("details: rerun with -v for full experiment/provenance evidence");
}

fn diagnosed_artifact_hashes<'a>(
    report: &'a CheckReport,
    variables: &[String],
    artifact: &std::path::Path,
) -> Option<(&'a str, &'a str, Option<(&'a str, bool)>)> {
    for result in &report.interventions {
        if !variables.iter().any(|variable| variable == &result.intervention.variable) {
            continue;
        }
        let Some(delta) = result
            .artifact_deltas
            .iter()
            .find(|delta| delta.changed && delta.logical_path == artifact)
        else {
            continue;
        };
        let reversion = result.confirmation_run.as_ref().and_then(|run| {
            run.artifacts
                .iter()
                .find(|candidate| candidate.logical_path == artifact)
                .map(|candidate| {
                    (
                        candidate.sha256.as_str(),
                        result.reverted_to_baseline == Some(true),
                    )
                })
        });
        return Some((
            delta.baseline_sha256.as_str(),
            delta.variant_sha256.as_str(),
            reversion,
        ));
    }

    if let Some(interaction) = &report.interaction_search {
        if variables
            .iter()
            .all(|variable| interaction.minimal_variables.contains(variable))
        {
            if let Some(delta) = interaction
                .artifact_deltas
                .iter()
                .find(|delta| delta.changed && delta.logical_path == artifact)
            {
                let reversion = interaction.confirmation_run.as_ref().and_then(|run| {
                    run.artifacts
                        .iter()
                        .find(|candidate| candidate.logical_path == artifact)
                        .map(|candidate| {
                            (
                                candidate.sha256.as_str(),
                                interaction.reverted_to_baseline == Some(true),
                            )
                        })
                });
                return Some((
                    delta.baseline_sha256.as_str(),
                    delta.variant_sha256.as_str(),
                    reversion,
                ));
            }
        }
    }

    None
}

fn intervention_summary_label(result: &crate::model::InterventionResult) -> &'static str {
    if result.error.is_some() && result.runs.is_empty() && result.build_failures.is_empty() {
        "ERROR"
    } else if !result.build_failures.is_empty() && !result.runs.is_empty() {
        "MIXED"
    } else if !result.build_failures.is_empty() {
        "BUILD FAILED"
    } else if result.changed {
        "EFFECT"
    } else {
        "no effect"
    }
}

fn reversion_summary_suffix(result: &crate::model::InterventionResult) -> &'static str {
    if !(result.changed || !result.build_failures.is_empty()) {
        return "";
    }
    match result.reverted_to_baseline {
        Some(true) => " [reversion confirmed]",
        Some(false) => " [reversion failed]",
        None => "",
    }
}

fn check_status_label(status: &CheckStatus) -> &'static str {
    match status {
        CheckStatus::Reproducible => "REPRODUCIBLE_WITHIN_TESTED_SPACE",
        CheckStatus::NonReproducible => "NON_REPRODUCIBLE",
        CheckStatus::UncontrolledNondeterminism => "UNCONTROLLED_NONDETERMINISM",
        CheckStatus::Inconclusive => "INCONCLUSIVE",
    }
}

pub fn print_compare_text(report: &EnvironmentComparisonReport, verbose: bool) {
    println!("ReproBisect known-environment comparison");
    println!("experiment: {}", report.experiment_id);
    println!("source sha256: {}", report.source_digest);
    println!("good manifest sha256: {}", report.good_manifest_sha256);
    println!("bad manifest sha256: {}", report.bad_manifest_sha256);
    println!(
        "good endpoint: {:?}  success={} failure={}",
        report.good_outcome_kind,
        report.good_runs.len(),
        report.good_failures.len()
    );
    println!(
        "bad endpoint: {:?}  success={} failure={}",
        report.bad_outcome_kind,
        report.bad_runs.len(),
        report.bad_failures.len()
    );

    if let Some(run) = report.good_runs.first() {
        println!(
            "runner: {:?}  image={} ({})",
            run.runner_backend,
            run.image,
            short_hash(&run.resolved_image_id)
        );
    } else if let Some(failure) = report.good_failures.first() {
        println!(
            "runner: {:?}  image={} ({})",
            failure.runner_backend,
            failure.image,
            short_hash(&failure.resolved_image_id)
        );
    }

    if let Some(signature) = &report.bad_failure_signature {
        println!(
            "known-bad failure signature: exit={} stdout_sha256={} stderr_sha256={}",
            signature.exit_code,
            short_hash(&signature.stdout_sha256),
            short_hash(&signature.stderr_sha256)
        );
    }

    println!("environment delta: {}", report.delta.len());
    for item in &report.delta {
        println!(
            "  {}: {} -> {}",
            item.variable, item.baseline_value, item.variant_value
        );
    }

    if !report.minimal_delta.is_empty() {
        println!("minimal observed bad-environment delta:");
        for item in &report.minimal_delta {
            println!(
                "  {}: {} -> {}",
                item.variable, item.baseline_value, item.variant_value
            );
        }
        println!("tested subsets: {}", report.tested_subsets);
        println!(
            "reproduction attempts: success={} failure={}",
            report.reproduction_runs.len(),
            report.reproduction_failures.len()
        );
        if let Some(reverted) = report.reverted_to_good {
            println!("good reversion recovered successful signature: {reverted}");
        }
    }

    if verbose {
        for run in &report.good_runs {
            print_run_logs(run);
        }
        for failure in &report.good_failures {
            print_failure_logs(failure);
        }
        for run in &report.bad_runs {
            print_run_logs(run);
        }
        for failure in &report.bad_failures {
            print_failure_logs(failure);
        }
        for run in &report.reproduction_runs {
            print_run_logs(run);
        }
        for failure in &report.reproduction_failures {
            print_failure_logs(failure);
        }
        if let Some(run) = &report.confirmation_run {
            print_run_logs(run);
        }
        if let Some(failure) = &report.confirmation_failure {
            print_failure_logs(failure);
        }
    }

    println!();
    match report.status {
        EnvironmentComparisonStatus::Equivalent => println!("status: ENVIRONMENTS_EQUIVALENT"),
        EnvironmentComparisonStatus::Minimized => println!("status: BAD_ENVIRONMENT_DELTA_MINIMIZED"),
        EnvironmentComparisonStatus::Inconclusive => println!("status: INCONCLUSIVE"),
    }
    for note in &report.notes {
        println!("note: {note}");
    }
}

pub fn print_compare_json(report: &EnvironmentComparisonReport) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(report)?);
    Ok(())
}

pub fn print_json(report: &CheckReport) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(report)?);
    Ok(())
}

fn print_run_logs(run: &crate::model::BuildRun) {
    if run.stdout_truncated || run.stderr_truncated {
        println!(
            "  log capture: cap={} bytes/stream stdout={} bytes{} stderr={} bytes{} (full stream SHA-256 retained)",
            run.log_capture_max_bytes,
            run.stdout_bytes,
            if run.stdout_truncated { " [text truncated]" } else { "" },
            run.stderr_bytes,
            if run.stderr_truncated { " [text truncated]" } else { "" },
        );
    }
    if !run.stdout.is_empty() {
        println!("  --- run {} stdout ---\n{}", run.ordinal, indent(&run.stdout, "  "));
    }
    if !run.stderr.is_empty() {
        println!("  --- run {} stderr ---\n{}", run.ordinal, indent(&run.stderr, "  "));
    }
}

fn print_failure_logs(failure: &crate::model::BuildFailure) {
    println!(
        "  --- failed run {} exit={} runner={:?} image={} ({}) ---",
        failure.ordinal,
        failure.exit_code,
        failure.runner_backend,
        failure.image,
        short_hash(&failure.resolved_image_id)
    );
    if failure.stdout_truncated || failure.stderr_truncated {
        println!(
            "  log capture: cap={} bytes/stream stdout={} bytes{} stderr={} bytes{} (full stream SHA-256 retained)",
            failure.log_capture_max_bytes,
            failure.stdout_bytes,
            if failure.stdout_truncated { " [text truncated]" } else { "" },
            failure.stderr_bytes,
            if failure.stderr_truncated { " [text truncated]" } else { "" },
        );
    }
    if !failure.stdout.is_empty() {
        println!(
            "  --- failed run {} stdout ---\n{}",
            failure.ordinal,
            indent(&failure.stdout, "  ")
        );
    }
    if !failure.stderr.is_empty() {
        println!(
            "  --- failed run {} stderr ---\n{}",
            failure.ordinal,
            indent(&failure.stderr, "  ")
        );
    }
}

fn print_toolchain_provenance(provenance: &crate::model::ToolchainProvenance) {
    if !provenance.probes.is_empty() {
        println!("  toolchain probes:");
        for probe in &provenance.probes {
            println!("    {}: {}", probe.tool, probe.version);
        }
    }
    if !provenance.bindings.is_empty() {
        println!("  controlled toolchain bindings:");
        for binding in &provenance.bindings {
            println!(
                "    {}: configured={} resolved={} version={} invocations={}",
                binding.variable,
                binding.configured_value,
                binding.resolved_path.as_deref().unwrap_or("<unresolved>"),
                binding.version.as_deref().unwrap_or("<unavailable>"),
                binding.invocation_count
            );
            if let Some(error) = &binding.error {
                println!("      note: {error}");
            }
        }
    }
    if let Some(error) = &provenance.error {
        println!("  toolchain probe note: {error}");
    }
}

fn print_source_overrides(overrides: &[crate::model::SourceOverrideRecord]) {
    if overrides.is_empty() {
        return;
    }
    println!("  source/dependency overrides:");
    for record in overrides {
        println!(
            "    {} <- {}  {} -> {}",
            record.target.display(),
            record.variant_source.display(),
            short_hash(&record.baseline_sha256),
            short_hash(&record.variant_sha256)
        );
    }
}

fn print_network_trace(trace: &crate::model::NetworkTraceSummary) {
    if !trace.attempted {
        return;
    }
    if trace.tracer_available {
        let scopes = trace
            .endpoint_scopes
            .iter()
            .map(|(scope, count)| format!("{scope}:{count}"))
            .collect::<Vec<_>>()
            .join(",");
        let successful_scopes = trace
            .successful_endpoint_scopes
            .iter()
            .map(|(scope, count)| format!("{scope}:{count}"))
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "  network trace: socket={} connect={} success={} failed={} sendto={} recvfrom={} families=[{}] scopes=[{}] successful_scopes=[{}] parsed_bytes={} truncated={}",
            trace.socket_calls,
            trace.connect_calls,
            trace.successful_connects,
            trace.failed_connects,
            trace.sendto_calls,
            trace.recvfrom_calls,
            trace.address_families.join(","),
            scopes,
            successful_scopes,
            trace.parsed_bytes,
            trace.trace_truncated,
        );
    } else if let Some(error) = &trace.error {
        println!("  network trace: unavailable ({error})");
    }
}

fn print_process_trace(trace: &crate::model::ProcessTraceSummary) {
    if !trace.attempted {
        return;
    }
    if !trace.tracer_available {
        if let Some(error) = &trace.error {
            println!("  process/file trace: unavailable ({error})");
        }
        return;
    }
    println!(
        "  process/file trace: processes={} parsed_bytes={} truncated={} same_process(dep+out={} cache+out={} net+cache={} net+cache+out={}) ancestor(dep+out={} cache+out={} net+out={} net+cache+out={}) mmap(dep={} cache={}) publication(out={} same_process_temp={} lineage_temp={})",
        trace.processes.len(),
        trace.parsed_bytes,
        trace.truncated,
        trace.same_process_dependency_output,
        trace.same_process_cache_output,
        trace.same_process_network_cache,
        trace.same_process_network_cache_output,
        trace.ancestor_dependency_output,
        trace.ancestor_cache_output,
        trace.ancestor_network_output,
        trace.ancestor_network_cache_output,
        trace.dependency_mmaps,
        trace.cache_mmaps,
        trace.output_publications,
        trace.temp_output_publications,
        trace.lineage_temp_output_publications,
    );
    for process in &trace.processes {
        println!(
            "    {} parent={} depth={} role={} dependency_reads={} dependency_mmaps={} cache_reads={} cache_mmaps={} output_writes={} output_publications={} temp_publications={} lineage_temp_publications={} successful_nonlocal_events={}",
            process.process,
            process.parent_process.as_deref().unwrap_or("-"),
            process.lineage_depth,
            process.role,
            process.dependency_reads,
            process.dependency_mmaps,
            process.cache_reads,
            process.cache_mmaps,
            process.output_writes,
            process.output_publications,
            process.temp_output_publications,
            process.lineage_temp_output_publications,
            process.successful_nonlocal_network_events,
        );
    }
    if trace.truncated {
        println!("    note: trace parsing was truncated; observed process correlations are incomplete");
    }
}

fn print_runtime_dependency_provenance(
    provenance: &crate::model::RuntimeDependencyProvenance,
) {
    if !provenance.attempted {
        return;
    }
    println!(
        "  runtime dependency provenance: declarations={} resolutions={} cache_summaries={}",
        provenance.dependency_files.len(),
        provenance.dependency_resolutions.len(),
        provenance.cache_summaries.len(),
    );
    for resolution in &provenance.dependency_resolutions {
        println!(
            "    {}: ecosystem={} packages={} normalized_sha256={}",
            resolution.path.display(),
            resolution.ecosystem,
            resolution.package_count,
            resolution
                .normalized_sha256
                .as_deref()
                .map(short_hash)
                .unwrap_or("<unavailable>")
        );
    }
    for cache in &provenance.cache_summaries {
        println!(
            "    cache {}: before(files={} bytes={} aggregate_sha256={} truncated={}) after(files={} bytes={} aggregate_sha256={} truncated={}) complete={} changed={} observed_change={} bounds(files={} bytes={})",
            cache.ecosystem,
            cache.before_file_count,
            cache.before_total_bytes,
            cache
                .before_aggregate_sha256
                .as_deref()
                .map(short_hash)
                .unwrap_or("<unavailable>"),
            cache.before_truncated,
            cache.after_file_count,
            cache.after_total_bytes,
            cache
                .after_aggregate_sha256
                .as_deref()
                .map(short_hash)
                .unwrap_or("<unavailable>"),
            cache.after_truncated,
            cache.comparison_complete,
            cache.changed_during_build,
            cache.observed_change,
            cache.max_files,
            cache.max_bytes,
        );
    }
    let correlation = &provenance.network_cache_correlation;
    if correlation.attempted {
        println!(
            "    network/cache build-window correlation: trace_available={} successful_nonlocal_events={} complete_cache_mutations={} incomplete_cache_observations={} cooccurrence={}",
            correlation.network_trace_available,
            correlation.successful_nonlocal_network_events,
            correlation.complete_cache_mutations,
            correlation.incomplete_cache_observations,
            correlation.build_window_cooccurrence,
        );
        if correlation.build_window_cooccurrence {
            println!("      note: co-occurrence does not identify which network response, if any, produced any cached byte");
        }
    }
    if let Some(error) = &provenance.error {
        println!("    note: {error}");
    }
}

fn confidence_label(confidence: &Confidence) -> &'static str {
    match confidence {
        Confidence::High => "HIGH",
        Confidence::Medium => "MEDIUM",
        Confidence::Low => "LOW",
    }
}

fn short_hash(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

fn indent(text: &str, prefix: &str) -> String {
    text.lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn print_fix_text(report: &crate::engine::FixReport, verbose: bool) {
    println!("ReproBisect fix planning");
    println!("diagnosis experiment: {}", report.diagnosis.experiment_id);
    println!("diagnosis status: {:?}", report.diagnosis.status);

    if report.candidates.is_empty() {
        println!("\nfix candidates: none");
    } else {
        println!("\nfix candidates:");
        for (index, candidate) in report.candidates.iter().enumerate() {
            println!("  {}. {}", index + 1, candidate.title);
            println!("     id: {}", candidate.id);
            println!("     causal variable: {}", candidate.causal_variable);
            println!("     strategy: {}", candidate.strategy);
            println!(
                "     automatic verification: {}",
                if candidate.automatic_verification_supported {
                    "SUPPORTED"
                } else {
                    "UNAVAILABLE"
                }
            );
            if !candidate.suggested_environment_appends.is_empty() {
                println!("     suggested flag appends:");
                for (key, value) in &candidate.suggested_environment_appends {
                    println!("       {key} += {value}");
                }
            }
            if !candidate.suggested_environment_overrides.is_empty() {
                println!("     suggested environment overrides:");
                for (key, value) in &candidate.suggested_environment_overrides {
                    println!("       {key}={value}");
                }
            }
            if !candidate.suggested_runner_controls.is_empty() {
                println!("     suggested runner controls:");
                for (key, value) in &candidate.suggested_runner_controls {
                    println!("       {key}={value}");
                }
            }
            if !candidate.suggested_shell_commands.is_empty() {
                println!("     suggested shell command(s):");
                for command in &candidate.suggested_shell_commands {
                    println!("       {command}");
                }
            }
            if let Some(patch) = &candidate.project_patch {
                println!("     project patch: {}", patch.path.display());
                println!(
                    "     expected sha256: {}",
                    patch.expected_sha256.as_deref().unwrap_or("<new file>")
                );
                println!("     unified diff:
{}", indent(&patch.unified_diff, "       "));
            }
            for evidence in &candidate.evidence {
                println!("     evidence: {evidence}");
            }
            for limitation in &candidate.limitations {
                println!("     limitation: {limitation}");
            }
        }
    }

    if !report.verifications.is_empty() {
        println!("\nverification:");
        for verification in &report.verifications {
            let status = if !verification.attempted {
                "NOT ATTEMPTED"
            } else if verification.verified {
                "VERIFIED"
            } else {
                "FAILED"
            };
            println!("  {}: {status}", verification.plan_id);
            if let Some(experiment_id) = verification.experiment_id {
                println!("    experiment: {experiment_id}");
            }
            for evidence in &verification.evidence {
                println!("    evidence: {evidence}");
            }
            if let Some(error) = &verification.error {
                println!("    error: {error}");
            }
            if verbose {
                for run in verification
                    .baseline_runs
                    .iter()
                    .chain(verification.intervention_runs.iter())
                {
                    print_run_logs(run);
                }
            }
        }
    }

    println!();
    for note in &report.notes {
        println!("note: {note}");
    }
}

pub fn print_fix_json(report: &crate::engine::FixReport) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(report)?);
    Ok(())
}
