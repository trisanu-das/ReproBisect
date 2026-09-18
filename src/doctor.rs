use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde::Serialize;

use crate::{config::Config, model::RunnerBackend};

pub const DOCTOR_REPORT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Pass,
    Warn,
    Fail,
    Skip,
}

impl DoctorStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
            Self::Skip => "SKIP",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub status: DoctorStatus,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub report_version: u32,
    pub project: PathBuf,
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runner: Option<RunnerBackend>,
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    fn new(project: &Path) -> Self {
        Self {
            report_version: DOCTOR_REPORT_VERSION,
            project: project.to_path_buf(),
            ready: true,
            runner: None,
            checks: Vec::new(),
        }
    }

    fn push(
        &mut self,
        name: impl Into<String>,
        status: DoctorStatus,
        detail: impl Into<String>,
        hint: Option<String>,
    ) {
        self.checks.push(DoctorCheck {
            name: name.into(),
            status,
            detail: detail.into(),
            hint,
        });
    }

    fn finish(&mut self) {
        self.ready = !self
            .checks
            .iter()
            .any(|check| check.status == DoctorStatus::Fail);
    }
}

pub fn inspect_project(project: &Path) -> DoctorReport {
    let mut report = DoctorReport::new(project);
    let config_path = project.join(".reprobisect.toml");

    if !config_path.is_file() {
        report.push(
            "configuration",
            DoctorStatus::Fail,
            format!("{} does not exist", config_path.display()),
            Some("run `reprobisect init .` or create .reprobisect.toml".to_string()),
        );
        report.finish();
        return report;
    }

    let config = match Config::load(project) {
        Ok(config) => config,
        Err(error) => {
            report.push(
                "configuration",
                DoctorStatus::Fail,
                format!("{error:#}"),
                Some("fix .reprobisect.toml before running a build".to_string()),
            );
            report.finish();
            return report;
        }
    };

    let runner = config.build.runner;
    report.runner = Some(runner);
    report.push(
        "configuration",
        DoctorStatus::Pass,
        format!(
            "valid config: {} runner, image {}, {} declared output(s)",
            runner.display_name(),
            config.build.image,
            config.build.outputs.len()
        ),
        None,
    );

    let outputs = config
        .build
        .outputs
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    report.push(
        "outputs",
        DoctorStatus::Pass,
        format!("declared artifact path(s): {outputs}"),
        None,
    );

    let executable = runner.executable();
    let executable_available = match run(executable, &["--version"]) {
        Ok(output) if output.status.success() => {
            let version = output_summary(&output);
            report.push(
                "runner executable",
                DoctorStatus::Pass,
                if version.is_empty() {
                    format!("{} is available", runner.display_name())
                } else {
                    version
                },
                None,
            );
            true
        }
        Ok(output) => {
            report.push(
                "runner executable",
                DoctorStatus::Fail,
                command_failure_detail(
                    &format!("{} executable returned a failure", runner.display_name()),
                    &output,
                ),
                Some(format!(
                    "verify `{} --version` works in this shell",
                    executable
                )),
            );
            false
        }
        Err(error) => {
            report.push(
                "runner executable",
                DoctorStatus::Fail,
                format!("cannot execute {executable}: {error}"),
                Some(format!(
                    "install {} or configure a different runner",
                    runner.display_name()
                )),
            );
            false
        }
    };

    if !executable_available {
        report.push(
            "runtime",
            DoctorStatus::Skip,
            "runner executable is unavailable",
            None,
        );
        report.push(
            "build image",
            DoctorStatus::Skip,
            "runtime check did not pass",
            None,
        );
        report.finish();
        return report;
    }

    let runtime_ready = match run(executable, &["info"]) {
        Ok(output) if output.status.success() => {
            report.push(
                "runtime",
                DoctorStatus::Pass,
                format!("{} runtime is reachable", runner.display_name()),
                None,
            );
            true
        }
        Ok(output) => {
            report.push(
                "runtime",
                DoctorStatus::Fail,
                command_failure_detail(
                    &format!("{} runtime is not ready", runner.display_name()),
                    &output,
                ),
                Some(runtime_hint(runner)),
            );
            false
        }
        Err(error) => {
            report.push(
                "runtime",
                DoctorStatus::Fail,
                format!("cannot query {} runtime: {error}", runner.display_name()),
                Some(runtime_hint(runner)),
            );
            false
        }
    };

    if runtime_ready {
        match run(executable, &["image", "inspect", &config.build.image]) {
            Ok(output) if output.status.success() => report.push(
                "build image",
                DoctorStatus::Pass,
                format!("{} is available locally", config.build.image),
                None,
            ),
            Ok(_) => report.push(
                "build image",
                DoctorStatus::Warn,
                format!("{} is not available locally", config.build.image),
                Some(format!(
                    "ReproBisect can use the runtime's normal pull behavior; optionally prefetch with `{} pull {}`",
                    executable, config.build.image
                )),
            ),
            Err(error) => report.push(
                "build image",
                DoctorStatus::Warn,
                format!("could not inspect image {}: {error}", config.build.image),
                Some(format!(
                    "try `{} image inspect {}` manually",
                    executable, config.build.image
                )),
            ),
        }
    } else {
        report.push(
            "build image",
            DoctorStatus::Skip,
            "runtime check did not pass",
            None,
        );
    }

    report.finish();
    report
}

pub fn print_text(report: &DoctorReport) {
    println!("ReproBisect doctor");
    println!("project: {}", report.project.display());
    if let Some(runner) = report.runner {
        println!("runner: {}", runner.display_name());
    }
    println!();

    for check in &report.checks {
        println!("[{}] {}: {}", check.status.label(), check.name, check.detail);
        if let Some(hint) = &check.hint {
            println!("       hint: {hint}");
        }
    }

    println!();
    println!(
        "ready: {}",
        if report.ready { "YES" } else { "NO" }
    );
}

fn run(executable: &str, args: &[&str]) -> std::io::Result<Output> {
    Command::new(executable).args(args).output()
}

fn output_summary(output: &Output) -> String {
    let stdout = first_nonempty_line(&output.stdout);
    if !stdout.is_empty() {
        return stdout;
    }
    first_nonempty_line(&output.stderr)
}

fn command_failure_detail(prefix: &str, output: &Output) -> String {
    let summary = output_summary(output);
    if summary.is_empty() {
        format!("{prefix} (exit status {})", output.status)
    } else {
        format!("{prefix}: {summary}")
    }
}

fn first_nonempty_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn runtime_hint(runner: RunnerBackend) -> String {
    match runner {
        RunnerBackend::Docker => {
            "start Docker and verify `docker info` succeeds for the current user".to_string()
        }
        RunnerBackend::Podman => {
            "verify `podman info` succeeds for the current user".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn missing_config_is_reported_as_not_ready() {
        let dir = tempdir().expect("tempdir");
        let report = inspect_project(dir.path());

        assert!(!report.ready);
        assert_eq!(report.checks.len(), 1);
        assert_eq!(report.checks[0].status, DoctorStatus::Fail);
        assert_eq!(report.checks[0].name, "configuration");
    }

    #[test]
    fn invalid_config_is_reported_without_invoking_a_runtime() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join(".reprobisect.toml"), "[build]\nimage = \"\"\n")
            .expect("write config");

        let report = inspect_project(dir.path());

        assert!(!report.ready);
        assert_eq!(report.checks.len(), 1);
        assert_eq!(report.checks[0].status, DoctorStatus::Fail);
    }

    #[test]
    fn warnings_do_not_make_a_report_unready() {
        let mut report = DoctorReport::new(Path::new("."));
        report.push(
            "image",
            DoctorStatus::Warn,
            "not cached",
            Some("it can be pulled".to_string()),
        );
        report.finish();

        assert!(report.ready);
    }

    #[test]
    fn first_nonempty_line_ignores_blank_lines() {
        assert_eq!(
            first_nonempty_line(b"\n  \nDocker version 28.0\nmore\n"),
            "Docker version 28.0"
        );
    }
}
