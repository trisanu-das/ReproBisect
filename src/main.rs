mod artifact;
mod ci;
mod cli;
mod compat;
mod config;
mod doctor;
mod engine;
mod environment;
mod evidence;
mod init;
mod model;
mod output_discovery;
mod report;
mod runner;
mod schema;
mod source;

use anyhow::Result;
use clap::{CommandFactory, FromArgMatches, Parser};
use cli::{Cli, Command};

const EXIT_ERROR: i32 = 5;

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Init(args) => cli::run_init(args),
        Command::Doctor(args) => cli::run_doctor(args),
        Command::Check(args) => cli::run_check(args, "check"),
        Command::Diagnose(args) => cli::run_check(args, "diagnose"),
        Command::Compare(args) => cli::run_compare(args),
        Command::Fix(args) => cli::run_fix(args),
        Command::Evidence(args) => cli::run_evidence(args),
    }
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let operation = match args.get(1).and_then(|s| s.to_str()) {
        Some("check") => Some("check"),
        Some("diagnose") => Some("diagnose"),
        _ => None,
    };
    let ci_requested =
        operation.is_some() && args.iter().take_while(|a| *a != "--").any(|a| a == "--ci");
    let parsed = if ci_requested {
        Cli::command()
            .color(clap::ColorChoice::Never)
            .try_get_matches_from(&args)
            .and_then(|matches| Cli::from_arg_matches(&matches))
    } else {
        Cli::try_parse_from(&args)
    };
    let cli = match parsed {
        Ok(cli) => cli,
        Err(error) if ci_requested && error.exit_code() != 0 => {
            let code = ci::finish_failure(&error.into(), 2, || {
                ci::print_failure(
                    "usage_error",
                    2,
                    operation.unwrap(),
                    ci::CiPolicy::default(),
                )
            });
            std::process::exit(code);
        }
        Err(error) => error.exit(),
    };
    let policy = match &cli.command {
        Command::Check(args) | Command::Diagnose(args) => args.ci_policy.unwrap_or_default(),
        _ => ci::CiPolicy::default(),
    };
    if let Err(error) = run(cli) {
        if ci_requested {
            let code = ci::finish_failure(&error, EXIT_ERROR, || {
                ci::print_failure("operational_error", EXIT_ERROR, operation.unwrap(), policy)
            });
            std::process::exit(code);
        } else {
            eprintln!("error: {error:#}");
        }
        std::process::exit(EXIT_ERROR);
    }
}
