use std::{fs, io::{self, Write}, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};

pub const EXIT_DIAGNOSTIC: i32 = 1;

use crate::{
    compat,
    config::Config,
    doctor,
    init,
    engine::{CompareOptions, CheckOptions, FixOptions, check_project, compare_environments, fix_project},
    environment::EnvironmentManifest,
    report,
};

#[derive(Debug, Parser)]
#[command(name = "reprobisect")]
#[command(version)]
#[command(about = "Diagnose non-reproducible builds through controlled rebuild experiments")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Detect the project and create an editable starter .reprobisect.toml.
    Init(InitArgs),

    /// Check project configuration and OCI runtime readiness without running a build.
    Doctor(DoctorArgs),

    /// Run controlled rebuilds plus configured one-variable causal interventions.
    Check(CheckArgs),

    /// Explicit alias for the same causal diagnosis pipeline as `check`.
    Diagnose(CheckArgs),

    /// Compare known-good and known-bad controlled environment manifests and minimize their delta.
    Compare(CompareArgs),

    /// Generate evidence-grounded fix candidates; optionally verify them experimentally.
    Fix(FixArgs),

    /// Validate and normalize persisted ReproBisect JSON evidence from supported historical schemas.
    Evidence(EvidenceArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Project directory.
    #[arg(default_value = ".")]
    pub project: PathBuf,

    /// Refuse to overwrite an existing config unless --force is passed.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Project directory containing .reprobisect.toml.
    #[arg(default_value = ".")]
    pub project: PathBuf,

    /// Output format.
    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Project directory containing .reprobisect.toml.
    #[arg(default_value = ".")]
    pub project: PathBuf,

    /// Output format.
    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Number of repeated controlled baseline builds. Must be at least 2.
    #[arg(long)]
    pub control_runs: Option<usize>,

    /// Number of builds for each environmental intervention. Must be at least 1.
    #[arg(long)]
    pub intervention_runs: Option<usize>,

    /// Number of baseline reversion confirmations after a detected effect (0 or 1).
    #[arg(long)]
    pub confirmation_runs: Option<usize>,

    /// Print full build stdout/stderr for each run.
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Debug, Args)]
pub struct CompareArgs {
    /// Project directory containing .reprobisect.toml.
    #[arg(default_value = ".")]
    pub project: PathBuf,

    /// Known-good environment manifest. Relative paths are resolved from the project root.
    #[arg(long)]
    pub good: PathBuf,

    /// Known-bad environment manifest. Relative paths are resolved from the project root.
    #[arg(long)]
    pub bad: PathBuf,

    /// Output format.
    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Repetitions for each known-good and known-bad endpoint. Must be at least 2.
    #[arg(long)]
    pub runs: Option<usize>,

    /// Repetitions for each ddmin subset predicate. Must be at least 2.
    #[arg(long)]
    pub subset_runs: Option<usize>,

    /// Re-run known-good after minimization (0 or 1).
    #[arg(long)]
    pub confirmation_runs: Option<usize>,

    /// Print full build stdout/stderr for endpoint, subset, and confirmation runs.
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Debug, Args)]
pub struct EvidenceArgs {
    /// Persisted ReproBisect JSON evidence file.
    pub input: PathBuf,

    /// Output format. JSON prints the normalized current-schema document.
    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Optionally write normalized current-schema JSON to this path.
    #[arg(long)]
    pub normalized_output: Option<PathBuf>,

    /// Allow --normalized-output to replace an existing file.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct FixArgs {
    /// Project directory containing .reprobisect.toml.
    #[arg(default_value = ".")]
    pub project: PathBuf,

    /// Output format.
    #[arg(long, value_enum, default_value = "text")]
    pub format: OutputFormat,

    /// Re-run the original failing intervention with each supported candidate fix.
    #[arg(long)]
    pub verify: bool,

    /// Number of repeated controlled baseline builds during diagnosis.
    #[arg(long)]
    pub control_runs: Option<usize>,

    /// Number of builds for each environmental intervention during diagnosis.
    #[arg(long)]
    pub intervention_runs: Option<usize>,

    /// Number of baseline reversion confirmations after a detected effect (0 or 1).
    #[arg(long)]
    pub confirmation_runs: Option<usize>,

    /// Print full diagnostic build logs in text mode.
    #[arg(short, long)]
    pub verbose: bool,
}

pub fn run_init(args: InitArgs) -> Result<()> {
    let project = fs::canonicalize(&args.project)
        .with_context(|| format!("cannot resolve project directory {}", args.project.display()))?;
    init::create_config(&project, args.force)?;
    Ok(())
}

pub fn run_doctor(args: DoctorArgs) -> Result<()> {
    let project = fs::canonicalize(&args.project)
        .with_context(|| format!("cannot resolve project directory {}", args.project.display()))?;
    let report_data = doctor::inspect_project(&project);

    match args.format {
        OutputFormat::Text => doctor::print_text(&report_data),
        OutputFormat::Json => {
            serde_json::to_writer_pretty(io::stdout().lock(), &report_data)
                .context("cannot print doctor JSON")?;
            println!();
        }
    }

    if !report_data.ready {
        io::stdout().flush().context("cannot flush doctor report output")?;
        std::process::exit(EXIT_DIAGNOSTIC);
    }

    Ok(())
}

pub fn run_check(args: CheckArgs) -> Result<()> {
    let project = fs::canonicalize(&args.project)
        .with_context(|| format!("cannot resolve project directory {}", args.project.display()))?;
    let config = Config::load(&project)?;

    let options = CheckOptions {
        control_runs: args.control_runs.unwrap_or(config.experiments.control_runs),
        intervention_runs: args
            .intervention_runs
            .unwrap_or(config.experiments.intervention_runs),
        confirmation_runs: args
            .confirmation_runs
            .unwrap_or(config.experiments.confirmation_runs),
        stochastic_runs: config.experiments.stochastic_runs,
        stochastic_alpha: config.experiments.stochastic_alpha,
        interaction_runs: config.experiments.interaction_runs,
        max_interaction_variables: config.experiments.max_interaction_variables,
    };

    validate_check_options(&options)?;

    let report_data = check_project(&project, &config, &options)?;

    match args.format {
        OutputFormat::Text => report::print_text(&report_data, args.verbose),
        OutputFormat::Json => report::print_json(&report_data)?,
    }

    let code = match report_data.status {
        crate::model::CheckStatus::Reproducible => 0,
        crate::model::CheckStatus::NonReproducible
        | crate::model::CheckStatus::UncontrolledNondeterminism
        | crate::model::CheckStatus::Inconclusive => EXIT_DIAGNOSTIC,
    };

    if code != 0 {
        io::stdout().flush().context("cannot flush report output")?;
        std::process::exit(code);
    }

    Ok(())
}


pub fn run_compare(args: CompareArgs) -> Result<()> {
    let project = fs::canonicalize(&args.project)
        .with_context(|| format!("cannot resolve project directory {}", args.project.display()))?;
    let config = Config::load(&project)?;
    let good_path = if args.good.is_absolute() {
        args.good.clone()
    } else {
        project.join(&args.good)
    };
    let bad_path = if args.bad.is_absolute() {
        args.bad.clone()
    } else {
        project.join(&args.bad)
    };
    let good = EnvironmentManifest::load(&good_path, &project)?;
    let bad = EnvironmentManifest::load(&bad_path, &project)?;
    let options = CompareOptions {
        endpoint_runs: args.runs.unwrap_or(config.experiments.comparison_runs),
        subset_runs: args
            .subset_runs
            .unwrap_or(config.experiments.comparison_subset_runs),
        confirmation_runs: args
            .confirmation_runs
            .unwrap_or(config.experiments.confirmation_runs),
    };

    let report_data = compare_environments(&project, &config, &good, &bad, &options)?;
    match args.format {
        OutputFormat::Text => report::print_compare_text(&report_data, args.verbose),
        OutputFormat::Json => report::print_compare_json(&report_data)?,
    }

    let code = match report_data.status {
        crate::model::EnvironmentComparisonStatus::Equivalent => 0,
        crate::model::EnvironmentComparisonStatus::Minimized
        | crate::model::EnvironmentComparisonStatus::Inconclusive => EXIT_DIAGNOSTIC,
    };
    if code != 0 {
        io::stdout().flush().context("cannot flush comparison report output")?;
        std::process::exit(code);
    }
    Ok(())
}

pub fn run_fix(args: FixArgs) -> Result<()> {
    let project = fs::canonicalize(&args.project)
        .with_context(|| format!("cannot resolve project directory {}", args.project.display()))?;
    let config = Config::load(&project)?;

    let check = CheckOptions {
        control_runs: args.control_runs.unwrap_or(config.experiments.control_runs),
        intervention_runs: args
            .intervention_runs
            .unwrap_or(config.experiments.intervention_runs),
        confirmation_runs: args
            .confirmation_runs
            .unwrap_or(config.experiments.confirmation_runs),
        stochastic_runs: config.experiments.stochastic_runs,
        stochastic_alpha: config.experiments.stochastic_alpha,
        interaction_runs: config.experiments.interaction_runs,
        max_interaction_variables: config.experiments.max_interaction_variables,
    };
    validate_check_options(&check)?;

    let fix_options = FixOptions {
        check,
        verify: args.verify,
    };
    let fix_report = fix_project(&project, &config, &fix_options)?;

    match args.format {
        OutputFormat::Text => report::print_fix_text(&fix_report, args.verbose),
        OutputFormat::Json => report::print_fix_json(&fix_report)?,
    }

    let success = if fix_report.diagnosis.status == crate::model::CheckStatus::Reproducible {
        true
    } else if args.verify {
        fix_report.verifications.iter().any(|verification| verification.verified)
    } else {
        !fix_report.candidates.is_empty()
    };

    if !success {
        io::stdout().flush().context("cannot flush fix report output")?;
        std::process::exit(EXIT_DIAGNOSTIC);
    }

    Ok(())
}

pub fn run_evidence(args: EvidenceArgs) -> Result<()> {
    let loaded = compat::load_evidence(&args.input)?;

    if let Some(path) = &args.normalized_output {
        compat::write_normalized(&loaded.source_path, path, &loaded.document, args.force)?;
    }

    match args.format {
        OutputFormat::Text => {
            println!("ReproBisect evidence compatibility check");
            println!("file: {}", loaded.source_path.display());
            println!("kind: {}", loaded.summary.kind.label());
            println!("source schema: {}", loaded.summary.source_schema_version);
            println!("current schema: {}", loaded.summary.normalized_schema_version);
            println!(
                "migration: {}",
                if loaded.summary.migrated { "APPLIED" } else { "NOT REQUIRED" }
            );
            println!(
                "build/run-failure records migrated: {}",
                loaded.summary.build_documents_migrated
            );
            println!(
                "legacy full-log identities derived: {}",
                loaded.summary.derived_legacy_log_digests
            );
            println!(
                "legacy cache summaries migrated: {}",
                loaded.summary.legacy_cache_summaries_migrated
            );
            for note in &loaded.summary.notes {
                println!("note: {note}");
            }
            if let Some(path) = &args.normalized_output {
                println!("normalized evidence: {}", path.display());
            }
        }
        OutputFormat::Json => {
            let value = loaded.document.to_value()?;
            serde_json::to_writer_pretty(io::stdout().lock(), &value)
                .context("cannot print normalized evidence JSON")?;
            println!();
        }
    }

    Ok(())
}

fn validate_check_options(options: &CheckOptions) -> Result<()> {
    if !(2..=32).contains(&options.control_runs) {
        bail!("control_runs must be between 2 and 32");
    }
    if !(1..=32).contains(&options.intervention_runs) {
        bail!("intervention_runs must be between 1 and 32");
    }
    if options.confirmation_runs > 1 {
        bail!("confirmation_runs currently supports only 0 or 1");
    }
    if !(3..=1024).contains(&options.stochastic_runs) {
        bail!("stochastic_runs must be between 3 and 1024");
    }
    if !(0.0 < options.stochastic_alpha && options.stochastic_alpha < 1.0) {
        bail!("stochastic_alpha must be strictly between 0 and 1");
    }
    if !(2..=32).contains(&options.interaction_runs) {
        bail!("interaction_runs must be between 2 and 32");
    }
    if !(2..=32).contains(&options.max_interaction_variables) {
        bail!("max_interaction_variables must be between 2 and 32");
    }
    Ok(())
}
