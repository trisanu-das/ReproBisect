mod artifact;
mod cli;
mod config;
mod doctor;
mod engine;
mod environment;
mod evidence;
mod model;
mod report;
mod schema;
mod compat;
mod runner;
mod source;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

const EXIT_ERROR: i32 = 5;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(EXIT_ERROR);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init(args) => cli::run_init(args),
        Command::Doctor(args) => cli::run_doctor(args),
        Command::Check(args) | Command::Diagnose(args) => cli::run_check(args),
        Command::Compare(args) => cli::run_compare(args),
        Command::Fix(args) => cli::run_fix(args),
        Command::Evidence(args) => cli::run_evidence(args),
    }
}
