mod oci;

use std::path::Path;

use anyhow::{Result, bail};
use uuid::Uuid;

use crate::model::{BuildFailure, BuildRun, BuildSpec, ControlledEnvironment, RunnerBackend};

pub use oci::OciRunner;

#[derive(Debug, Clone)]
pub enum RunOutcome {
    Success(BuildRun),
    BuildFailed(BuildFailure),
}

pub trait Runner {
    /// Execute one build attempt. A non-zero build-command exit is returned as
    /// structured evidence; runner/infrastructure failures remain `Err`.
    fn run_attempt(
        &self,
        spec: &BuildSpec,
        experiment_id: Uuid,
        source_digest: &str,
        ordinal: usize,
        environment: &ControlledEnvironment,
    ) -> Result<RunOutcome>;

    /// Compatibility helper for experiment types where a non-zero build exit is
    /// not itself an expected observable. Callers interested in hermeticity
    /// should use `run_attempt` so build failures are not conflated with OCI-runtime
    /// or orchestration errors.
    fn run(
        &self,
        spec: &BuildSpec,
        experiment_id: Uuid,
        source_digest: &str,
        ordinal: usize,
        environment: &ControlledEnvironment,
    ) -> Result<BuildRun> {
        match self.run_attempt(spec, experiment_id, source_digest, ordinal, environment)? {
            RunOutcome::Success(run) => Ok(run),
            RunOutcome::BuildFailed(failure) => bail!(
                "build run {} failed with exit code {}\nstdout:\n{}\nstderr:\n{}",
                failure.ordinal,
                failure.exit_code,
                failure.stdout,
                failure.stderr
            ),
        }
    }

    fn available(&self) -> Result<()>;
}

pub fn configured_runner(project_root: &Path, backend: RunnerBackend) -> OciRunner {
    OciRunner::new(project_root.to_path_buf(), backend)
}
