use anyhow::{Context, Result};

use crate::model::CheckStatus;
use clap::ValueEnum;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum CiPolicy {
    #[default]
    RequireReproducible,
    ReportOnly,
}

#[derive(Debug, Serialize)]
pub struct PolicyResult {
    pub name: CiPolicy,
    pub passed: bool,
    pub diagnostic_exit_code: i32,
    pub exit_code: i32,
}

pub fn evaluate_policy(status: &CheckStatus, complete: bool, policy: CiPolicy) -> PolicyResult {
    let diagnostic_exit_code = if *status == CheckStatus::Reproducible {
        0
    } else {
        1
    };
    let passed = complete && (matches!(policy, CiPolicy::ReportOnly) || diagnostic_exit_code == 0);
    PolicyResult {
        name: policy,
        passed,
        diagnostic_exit_code,
        exit_code: if passed { 0 } else { 1 },
    }
}

#[derive(Debug, Serialize)]
pub struct Completion {
    pub completed: bool,
    pub reason: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Identities {
    pub binary_version: &'static str,
    pub binary_sha256: Option<String>,
    pub effective_config_sha256: Option<String>,
    pub source_sha256: Option<String>,
    pub experiment_id: Option<uuid::Uuid>,
}

#[derive(Debug, Serialize)]
pub struct ReportReference {
    pub path: String,
    pub sha256: String,
    pub schema_version: u32,
}

#[derive(Debug, Serialize)]
pub struct Counts {
    pub planned: usize,
    pub attempted: usize,
    pub completed: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[derive(Debug, Serialize)]
pub struct InteractionCoverage {
    pub status: &'static str,
    pub strategy: &'static str,
    pub eligible_candidates: usize,
    pub tested_subsets: Option<usize>,
}

fn interaction_coverage(
    report: &crate::model::CheckReport,
    config: &crate::config::Config,
    options: &crate::engine::CheckOptions,
    plan: &[crate::engine::interventions::PlannedIntervention],
) -> InteractionCoverage {
    let eligible = plan
        .iter()
        .filter(|p| {
            report.interventions.iter().any(|r| {
                r.intervention.id == p.intervention.id
                    && crate::engine::interventions::is_interaction_candidate(p, r)
            })
        })
        .count();
    let status = if !config.experiments.interaction_search {
        "disabled"
    } else if report.status == CheckStatus::UncontrolledNondeterminism {
        "blocked_unstable_baseline"
    } else if eligible < 2 {
        "not_applicable"
    } else if eligible > options.max_interaction_variables {
        "skipped_limit"
    } else if let Some(search) = &report.interaction_search {
        let confirmation = options.confirmation_runs == 0
            || !(search.changed && search.stable_effect)
            || search.confirmation_run.is_some();
        if search.error.is_none()
            && search.runs.len() == options.interaction_runs
            && search.tested_subsets > 0
            && confirmation
        {
            "completed"
        } else {
            "failed"
        }
    } else {
        "failed_or_missing"
    };
    InteractionCoverage {
        status,
        strategy: "combined_candidate_set_then_ddmin_not_exhaustive",
        eligible_candidates: eligible,
        tested_subsets: report.interaction_search.as_ref().map(|s| s.tested_subsets),
    }
}

#[derive(Debug, Serialize)]
pub struct Coverage {
    pub complete: bool,
    pub controls: Counts,
    pub interventions: Counts,
    pub interaction_search: InteractionCoverage,
}

fn result_complete(
    result: &crate::model::InterventionResult,
    options: &crate::engine::CheckOptions,
) -> bool {
    let planned =
        crate::engine::interventions::intervention_trial_count(result.intervention.kind, options);
    let needs_reference = result.intervention.kind == crate::model::InterventionKind::CpuCount;
    let needs_confirmation =
        options.confirmation_runs > 0 && (result.changed || !result.build_failures.is_empty());
    result.error.is_none()
        && result.runs.len() + result.build_failures.len() == planned
        && (!needs_reference || result.reference_runs.len() == planned)
        && (!needs_confirmation || result.confirmation_run.is_some())
}

fn coverage(
    report: &crate::model::CheckReport,
    config: &crate::config::Config,
    options: &crate::engine::CheckOptions,
) -> Coverage {
    let baseline = crate::engine::interventions::baseline_environment(config);
    let plan = crate::engine::interventions::plan_interventions(config, &baseline);
    let attempted = plan
        .iter()
        .filter(|p| {
            report
                .interventions
                .iter()
                .any(|r| r.intervention.id == p.intervention.id)
        })
        .count();
    let completed = plan
        .iter()
        .filter(|p| {
            report
                .interventions
                .iter()
                .any(|r| r.intervention.id == p.intervention.id && result_complete(r, options))
        })
        .count();
    let controls = report.runs.len();
    let interaction_search = interaction_coverage(report, config, options, &plan);
    let interaction_complete = matches!(
        interaction_search.status,
        "disabled" | "not_applicable" | "completed"
    );
    Coverage {
        complete: controls == options.control_runs
            && completed == plan.len()
            && interaction_complete
            && report.status != CheckStatus::UncontrolledNondeterminism,
        controls: Counts {
            planned: options.control_runs,
            attempted: controls,
            completed: controls,
            skipped: options.control_runs.saturating_sub(controls),
            failed: 0,
        },
        interventions: Counts {
            planned: plan.len(),
            attempted,
            completed,
            skipped: plan.len() - attempted,
            failed: attempted - completed,
        },
        interaction_search,
    }
}

#[derive(Debug, Serialize)]
pub struct CiEnvelope {
    pub schema_version: u32,
    pub operation: &'static str,
    pub diagnostic_status: Option<CheckStatus>,
    pub completion: Completion,
    pub identities: Identities,
    pub report: Option<ReportReference>,
    pub coverage: Option<Coverage>,
    pub policy: PolicyResult,
}

pub fn from_report(
    project: &std::path::Path,
    report: &crate::model::CheckReport,
    config: &crate::config::Config,
    options: &crate::engine::CheckOptions,
    policy: CiPolicy,
) -> Result<CiEnvelope> {
    use sha2::{Digest, Sha256};
    let relative = format!(".reprobisect/experiments/{}.json", report.experiment_id);
    let bytes =
        std::fs::read(project.join(&relative)).context("cannot read persisted CI report")?;
    let mut expected = serde_json::to_vec_pretty(report)?;
    expected.push(b'\n');
    anyhow::ensure!(
        bytes == expected,
        "persisted CI report does not match returned evidence"
    );
    let binary =
        std::fs::read(std::env::current_exe()?).context("cannot identify running binary")?;
    let mut effective_config = config.clone();
    effective_config.experiments.control_runs = options.control_runs;
    effective_config.experiments.intervention_runs = options.intervention_runs;
    effective_config.experiments.confirmation_runs = options.confirmation_runs;
    effective_config.experiments.stochastic_runs = options.stochastic_runs;
    effective_config.experiments.stochastic_alpha = options.stochastic_alpha;
    effective_config.experiments.interaction_runs = options.interaction_runs;
    effective_config.experiments.max_interaction_variables = options.max_interaction_variables;
    let effective = serde_json::to_vec(&effective_config)?;
    let coverage = coverage(report, config, options);
    let policy = evaluate_policy(&report.status, coverage.complete, policy);
    let reason = if coverage.complete {
        "completed"
    } else {
        "incomplete_coverage"
    };
    Ok(CiEnvelope {
        schema_version: 1,
        operation: "check",
        diagnostic_status: Some(report.status.clone()),
        completion: Completion {
            completed: coverage.complete,
            reason,
        },
        identities: Identities {
            binary_version: env!("CARGO_PKG_VERSION"),
            binary_sha256: Some(hex::encode(Sha256::digest(binary))),
            effective_config_sha256: Some(hex::encode(Sha256::digest(effective))),
            source_sha256: Some(report.source_digest.clone()),
            experiment_id: Some(report.experiment_id),
        },
        report: Some(ReportReference {
            path: relative,
            sha256: hex::encode(Sha256::digest(bytes)),
            schema_version: report.schema_version,
        }),
        coverage: Some(coverage),
        policy,
    })
}

pub fn print_failure(
    reason: &'static str,
    exit_code: i32,
    operation: &'static str,
    policy: CiPolicy,
) -> Result<()> {
    let envelope = CiEnvelope {
        schema_version: 1,
        operation,
        diagnostic_status: None,
        completion: Completion {
            completed: false,
            reason,
        },
        identities: Identities {
            binary_version: env!("CARGO_PKG_VERSION"),
            binary_sha256: None,
            effective_config_sha256: None,
            source_sha256: None,
            experiment_id: None,
        },
        report: None,
        coverage: None,
        policy: PolicyResult {
            name: policy,
            passed: false,
            diagnostic_exit_code: exit_code,
            exit_code,
        },
    };
    crate::report::print_automation_json(&envelope)
}

pub fn finish_failure(
    error: &anyhow::Error,
    original_exit: i32,
    emit: impl FnOnce() -> Result<()>,
) -> i32 {
    write_diagnostic(&format_args!("error: {error:#}"));
    if error.is::<crate::report::AutomationOutputError>() {
        return 5;
    }
    match emit() {
        Ok(()) => original_exit,
        Err(error) => {
            write_diagnostic(&format_args!("error: {error:#}"));
            5
        }
    }
}

/// Diagnostics are best-effort: a failed stderr transport must not prevent stdout.
pub fn write_diagnostic(error: &dyn std::fmt::Display) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "{}", diagnostic(error));
}

/// Keep local CI diagnostics bounded and without terminal control bytes.
pub fn diagnostic(error: &dyn std::fmt::Display) -> String {
    error
        .to_string()
        .chars()
        .filter(|c| !c.is_control())
        .take(4096)
        .collect()
}

#[cfg(test)]
struct FailingWriter {
    bytes: Vec<u8>,
    writes: usize,
    fail_flush: bool,
}
#[cfg(test)]
impl std::io::Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.writes += 1;
        if !self.fail_flush && self.writes == 2 {
            return Err(std::io::Error::other("injected partial write failure"));
        }
        let count = if !self.fail_flush && self.writes == 1 {
            17
        } else {
            bytes.len()
        };
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if self.fail_flush {
            Err(std::io::Error::other("injected flush failure"))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CheckStatus;
    use sha2::Digest;

    fn sample() -> (
        crate::model::CheckReport,
        crate::config::Config,
        crate::engine::CheckOptions,
    ) {
        let loaded = crate::compat::load_evidence(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/evidence/phase6-check-v5.json"
        )))
        .unwrap();
        let crate::compat::EvidenceDocument::CheckReport(mut report) = loaded.document else {
            panic!("expected check report")
        };
        report.status = CheckStatus::Reproducible;
        report.interventions.clear();
        report.interaction_search = None;
        let config: crate::config::Config = toml::from_str(
            r#"
[build]
image = "test:local"
command = ["true"]
outputs = ["app"]
[experiments.dimensions]
build_path = false
source_date_epoch = false
timezone = false
locale = false
hostname = false
"#,
        )
        .unwrap();
        let options = crate::engine::CheckOptions {
            control_runs: report.runs.len(),
            intervention_runs: 2,
            confirmation_runs: 1,
            stochastic_runs: 8,
            stochastic_alpha: 0.05,
            interaction_runs: 2,
            max_interaction_variables: 8,
        };
        (report, config, options)
    }

    fn result_for(
        config: &crate::config::Config,
        run: &crate::model::BuildRun,
        count: usize,
    ) -> crate::model::InterventionResult {
        let baseline = crate::engine::interventions::baseline_environment(config);
        let planned = crate::engine::interventions::plan_interventions(config, &baseline);
        crate::model::InterventionResult {
            intervention: planned[0].intervention.clone(),
            runs: vec![run.clone(); count],
            reference_runs: vec![],
            build_failures: vec![],
            artifact_deltas: vec![],
            changed: false,
            variant_stable: Some(true),
            distinct_variant_outcomes: 1,
            stochastic_effect: None,
            reverted_to_baseline: None,
            confirmation_run: None,
            error: None,
        }
    }

    #[test]
    fn ci_changed_singles_are_outside_interaction_candidates() {
        let (mut report, mut config, mut options) = sample();
        config.experiments.dimensions.timezone = true;
        config.experiments.dimensions.locale = true;
        config.experiments.dimensions.hostname = true;
        config.experiments.interaction_search = true;
        report.status = CheckStatus::NonReproducible;
        let baseline = crate::engine::interventions::baseline_environment(&config);
        let plan = crate::engine::interventions::plan_interventions(&config, &baseline);
        report.interventions = plan
            .iter()
            .map(|p| {
                let mut result = result_for(&config, &report.runs[0], options.intervention_runs);
                result.intervention = p.intervention.clone();
                result
            })
            .collect();
        // Two changed, confirmed singles leave one candidate: no search is required,
        // even when the full plan exceeds the cap.
        for result in &mut report.interventions[..2] {
            result.changed = true;
            result.confirmation_run = Some(report.runs[0].clone());
        }
        options.max_interaction_variables = 2;
        let actual = coverage(&report, &config, &options);
        assert_eq!(actual.interaction_search.eligible_candidates, 1);
        assert_eq!(actual.interaction_search.status, "not_applicable");
        assert!(actual.complete);
        assert_eq!(
            evaluate_policy(&report.status, actual.complete, CiPolicy::ReportOnly).exit_code,
            0
        );

        // One changed single must not inflate a completed two-candidate search
        // over its cap. Stability is not an extra engine eligibility condition.
        report.interventions[1].changed = false;
        report.interventions[1].variant_stable = Some(false);
        report.interaction_search = Some(crate::model::InteractionSearchResult {
            candidate_variables: plan[1..]
                .iter()
                .map(|p| p.intervention.variable.clone())
                .collect(),
            minimal_variables: vec![],
            tested_subsets: 1,
            runs: vec![report.runs[0].clone(); options.interaction_runs],
            artifact_deltas: vec![],
            changed: false,
            stable_effect: false,
            reverted_to_baseline: None,
            confirmation_run: None,
            error: None,
        });
        let actual = coverage(&report, &config, &options);
        assert_eq!(actual.interaction_search.eligible_candidates, 2);
        assert_eq!(actual.interaction_search.status, "completed");
        assert!(actual.complete);
        assert_eq!(
            evaluate_policy(&report.status, actual.complete, CiPolicy::ReportOnly).exit_code,
            0
        );
        options.max_interaction_variables = 1;
        let actual = coverage(&report, &config, &options);
        assert_eq!(actual.interaction_search.status, "skipped_limit");
        assert!(!actual.complete);
    }

    #[test]
    fn ci_interaction_skips_never_claim_complete_coverage() {
        let (mut report, mut config, mut options) = sample();
        config.experiments.dimensions.timezone = true;
        config.experiments.dimensions.locale = true;
        config.experiments.interaction_search = true;
        let baseline = crate::engine::interventions::baseline_environment(&config);
        let plan = crate::engine::interventions::plan_interventions(&config, &baseline);
        report.interventions = plan
            .iter()
            .map(|p| {
                let mut result = result_for(&config, &report.runs[0], options.intervention_runs);
                result.intervention = p.intervention.clone();
                result
            })
            .collect();
        options.max_interaction_variables = 1;
        assert!(!coverage(&report, &config, &options).complete);
        options.max_interaction_variables = 8;
        assert!(!coverage(&report, &config, &options).complete);
        report.interaction_search = Some(crate::model::InteractionSearchResult {
            candidate_variables: plan
                .iter()
                .map(|p| p.intervention.variable.clone())
                .collect(),
            minimal_variables: vec![],
            tested_subsets: 1,
            runs: vec![report.runs[0].clone(); options.interaction_runs],
            artifact_deltas: vec![],
            changed: false,
            stable_effect: false,
            reverted_to_baseline: None,
            confirmation_run: None,
            error: None,
        });
        assert!(coverage(&report, &config, &options).complete);
        report.interaction_search.as_mut().unwrap().changed = true;
        report.interaction_search.as_mut().unwrap().stable_effect = true;
        assert!(!coverage(&report, &config, &options).complete);
        report.interaction_search.as_mut().unwrap().confirmation_run = Some(report.runs[0].clone());
        assert!(coverage(&report, &config, &options).complete);
    }

    #[test]
    fn ci_completed_results_require_all_trials() {
        let (mut report, mut config, options) = sample();
        config.experiments.dimensions.timezone = true;
        report.interventions = vec![result_for(&config, &report.runs[0], 1)];
        assert!(!coverage(&report, &config, &options).complete);
        report.interventions[0].runs.push(report.runs[0].clone());
        assert!(coverage(&report, &config, &options).complete);
        report.interventions[0].changed = true;
        assert!(!coverage(&report, &config, &options).complete);
        report.interventions[0].confirmation_run = Some(report.runs[0].clone());
        assert!(coverage(&report, &config, &options).complete);
        report.interventions[0].error = Some("confirmation failed".into());
        assert!(!coverage(&report, &config, &options).complete);

        config.experiments.dimensions.timezone = false;
        config.experiments.dimensions.cpu_count = true;
        report.interventions = vec![result_for(
            &config,
            &report.runs[0],
            options.stochastic_runs,
        )];
        assert!(!coverage(&report, &config, &options).complete);
        report.interventions[0].reference_runs =
            vec![report.runs[0].clone(); options.stochastic_runs];
        assert!(coverage(&report, &config, &options).complete);
    }

    #[test]
    fn ci_equivalent_toml_and_cli_overrides_have_same_identity() {
        use clap::Parser;
        let temp = tempfile::tempdir().unwrap();
        let (report, config, mut options) = sample();
        crate::evidence::persist_report(temp.path(), &report).unwrap();
        let cli = crate::cli::Cli::try_parse_from([
            "reprobisect",
            "check",
            "--ci",
            "--control-runs",
            "3",
            "--intervention-runs",
            "4",
            "--confirmation-runs",
            "0",
        ])
        .unwrap();
        let crate::cli::Command::Check(args) = cli.command else {
            panic!("expected check")
        };
        options.control_runs = args.control_runs.unwrap();
        options.intervention_runs = args.intervention_runs.unwrap();
        options.confirmation_runs = args.confirmation_runs.unwrap();
        let overridden = from_report(
            temp.path(),
            &report,
            &config,
            &options,
            CiPolicy::ReportOnly,
        )
        .unwrap();
        let mut effective_config = config.clone();
        effective_config.experiments.control_runs = options.control_runs;
        effective_config.experiments.intervention_runs = options.intervention_runs;
        effective_config.experiments.confirmation_runs = options.confirmation_runs;
        // Round-trip real TOML, with no CLI override needed for these settings.
        let toml_config: crate::config::Config =
            toml::from_str(&toml::to_string(&effective_config).unwrap()).unwrap();
        let configured = from_report(
            temp.path(),
            &report,
            &toml_config,
            &options,
            CiPolicy::ReportOnly,
        )
        .unwrap();
        assert_eq!(
            overridden.identities.effective_config_sha256,
            configured.identities.effective_config_sha256
        );
        for field in 0..7 {
            let mut changed = options.clone();
            match field {
                0 => changed.control_runs += 1,
                1 => changed.intervention_runs += 1,
                2 => changed.confirmation_runs = 1,
                3 => changed.stochastic_runs += 1,
                4 => changed.stochastic_alpha = 0.01,
                5 => changed.interaction_runs += 1,
                _ => changed.max_interaction_variables += 1,
            }
            let envelope = from_report(
                temp.path(),
                &report,
                &config,
                &changed,
                CiPolicy::ReportOnly,
            )
            .unwrap();
            assert_ne!(
                overridden.identities.effective_config_sha256,
                envelope.identities.effective_config_sha256
            );
        }
        effective_config.build.image = "different:local".into();
        let different = from_report(
            temp.path(),
            &report,
            &effective_config,
            &options,
            CiPolicy::ReportOnly,
        )
        .unwrap();
        assert_ne!(
            overridden.identities.effective_config_sha256,
            different.identities.effective_config_sha256
        );
    }

    #[test]
    fn ci_partial_coverage_never_passes() {
        let temp = tempfile::tempdir().unwrap();
        let (mut report, mut config, options) = sample();
        config.experiments.dimensions.timezone = true;
        // The engine can return early before any configured candidates run.
        for status in [
            CheckStatus::UncontrolledNondeterminism,
            CheckStatus::Reproducible,
        ] {
            report.status = status.clone();
            report.experiment_id = uuid::Uuid::new_v4();
            crate::evidence::persist_report(temp.path(), &report).unwrap();
            for policy in [CiPolicy::RequireReproducible, CiPolicy::ReportOnly] {
                let envelope =
                    from_report(temp.path(), &report, &config, &options, policy).unwrap();
                let value = serde_json::to_value(envelope).unwrap();
                assert_eq!(
                    value["diagnostic_status"],
                    serde_json::to_value(&status).unwrap()
                );
                assert_eq!(value["coverage"]["interventions"]["planned"], 1);
                assert_eq!(value["coverage"]["interventions"]["skipped"], 1);
                assert_eq!(value["coverage"]["complete"], false);
                assert_eq!(value["completion"]["completed"], false);
                assert_eq!(value["policy"]["passed"], false);
                assert_eq!(value["policy"]["exit_code"], 1);
            }
        }
    }

    #[test]
    fn ci_diagnostic_is_unicode_bounded_and_omits_controls() {
        let error = anyhow::anyhow!("root\0\u{1b}\r\n\t cause").context("configuration failed");
        let message = diagnostic(&format_args!("{error:#}"));
        assert!(message.contains("configuration failed"));
        assert!(message.contains("root"));
        assert!(message.contains("cause"));
        assert!(!message.chars().any(char::is_control));
        let message = diagnostic(&"🦀".repeat(5000));
        assert_eq!(message.chars().count(), 4096);
    }

    #[test]
    fn ci_output_failure_never_appends_a_replacement_document() {
        let temp = tempfile::tempdir().unwrap();
        let (report, config, options) = sample();
        crate::evidence::persist_report(temp.path(), &report).unwrap();
        let envelope = from_report(
            temp.path(),
            &report,
            &config,
            &options,
            CiPolicy::ReportOnly,
        )
        .unwrap();
        let mut expected = serde_json::to_vec(&envelope).unwrap();
        expected.push(b'\n');
        let mut success = vec![];
        crate::report::write_automation_json(&envelope, &mut success).unwrap();
        assert_eq!(success, expected);
        for fail_flush in [false, true] {
            let mut writer = FailingWriter {
                bytes: vec![],
                writes: 0,
                fail_flush,
            };
            let error = crate::report::write_automation_json(&envelope, &mut writer).unwrap_err();
            let committed = writer.bytes.clone();
            assert_eq!(
                committed,
                if fail_flush {
                    expected.clone()
                } else {
                    expected[..17].to_vec()
                }
            );
            let mut replacement_attempted = false;
            // This is the same handler used by main after run_check propagation.
            let code = finish_failure(&error.context("outer context"), 5, || {
                replacement_attempted = true;
                crate::report::write_automation_json(&envelope, &mut writer)
            });
            assert_eq!(code, 5);
            assert_eq!(
                writer.bytes, committed,
                "a recovered transport must not receive a second document"
            );
            assert!(!replacement_attempted);
        }
    }

    #[test]
    fn ci_missing_report_is_not_success() {
        let temp = tempfile::tempdir().unwrap();
        let (report, config, options) = sample();
        assert!(
            from_report(
                temp.path(),
                &report,
                &config,
                &options,
                CiPolicy::ReportOnly
            )
            .is_err()
        );
        let path = crate::evidence::persist_report(temp.path(), &report).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let envelope = from_report(
            temp.path(),
            &report,
            &config,
            &options,
            CiPolicy::ReportOnly,
        )
        .unwrap();
        let value = serde_json::to_value(envelope).unwrap();
        assert_eq!(
            value["report"]["sha256"],
            hex::encode(sha2::Sha256::digest(&bytes))
        );
        assert_eq!(value["diagnostic_status"], "reproducible");
        assert_eq!(value["identities"]["source_sha256"], report.source_digest);
        assert_eq!(
            value["identities"]["experiment_id"],
            report.experiment_id.to_string()
        );
        assert_eq!(
            value["identities"]["binary_sha256"].as_str().unwrap().len(),
            64
        );
        assert_eq!(
            value["identities"]["effective_config_sha256"]
                .as_str()
                .unwrap()
                .len(),
            64
        );
        std::fs::write(path, b"{}").unwrap();
        assert!(
            from_report(
                temp.path(),
                &report,
                &config,
                &options,
                CiPolicy::ReportOnly
            )
            .is_err()
        );
    }

    #[test]
    fn ci_policy_preserves_diagnostic_status() {
        for (status, historical_code) in [
            (CheckStatus::Reproducible, 0),
            (CheckStatus::NonReproducible, 1),
            (CheckStatus::UncontrolledNondeterminism, 1),
            (CheckStatus::Inconclusive, 1),
        ] {
            let strict = evaluate_policy(&status, true, CiPolicy::RequireReproducible);
            let informational = evaluate_policy(&status, true, CiPolicy::ReportOnly);
            assert_eq!(strict.diagnostic_exit_code, historical_code);
            assert_eq!(informational.diagnostic_exit_code, historical_code);
            assert_eq!(strict.passed, historical_code == 0);
            assert!(informational.passed);
        }
    }
}
