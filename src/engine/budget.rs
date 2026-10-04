//! Operation-scoped allowance. Reservations are never refunded: lost results
//! and cancelled work remain charged. This is not a persistent resume journal.
use anyhow::{Result, bail};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn install_signal_handler() -> Result<()> {
    ctrlc::set_handler(|| INTERRUPTED.store(true, std::sync::atomic::Ordering::SeqCst))?;
    Ok(())
}
#[derive(serde::Serialize)]
struct OwnershipRecord {
    container_name: String,
    purpose: &'static str,
    container_id: Option<String>,
    creation_outcome: &'static str,
    cleanup_verified: bool,
}

#[derive(Clone)]
pub struct ExecutionContext {
    state: Arc<Mutex<State>>,
    id: uuid::Uuid,
    started: Instant,
    ignore_signal: bool,
    pub resources: Arc<crate::config::ResourcePolicy>,
    prepaid: Option<Arc<Mutex<u64>>>,
    prepaid_infrastructure: Option<Arc<Mutex<u64>>>,
}
struct State {
    max: u64,
    charged: u64,
    reason: Option<&'static str>,
    deadline: Instant,
    owned: Vec<OwnershipRecord>,
    partial_attempts: Vec<String>,
}
impl ExecutionContext {
    pub fn new(config: &crate::config::ExecutionConfig) -> Result<Self> {
        config.validate()?;
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(config.total_timeout_seconds))
            .ok_or_else(|| anyhow::anyhow!("total deadline overflows monotonic clock"))?;
        Ok(Self {
            id: uuid::Uuid::new_v4(),
            started: Instant::now(),
            ignore_signal: false,
            state: Arc::new(Mutex::new(State {
                max: config.max_dispatches,
                charged: 0,
                reason: None,
                deadline,
                owned: Vec::new(),
                partial_attempts: Vec::new(),
            })),
            prepaid: None,
            prepaid_infrastructure: None,
            resources: Arc::new(config.resources.effective()),
        })
    }
    pub fn reserve(&self, count: u64) -> Result<()> {
        if !self.ignore_signal && INTERRUPTED.load(std::sync::atomic::Ordering::SeqCst) {
            self.cancel();
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("execution lock poisoned"))?;
        if Instant::now() >= state.deadline {
            state.reason.get_or_insert("deadline_exhausted");
        }
        if state.reason.is_some() || count > state.max.saturating_sub(state.charged) {
            state.reason.get_or_insert("budget_exhausted");
            bail!("{}", state.reason.unwrap());
        }
        state.charged += count;
        Ok(())
    }
    pub fn validate_environment(
        &self,
        environment: &crate::model::ControlledEnvironment,
    ) -> Result<()> {
        if self.resources.restricted && (environment.network_trace || environment.file_input_trace)
        {
            bail!("restricted resource profile refuses tracing without a temporary disk cap");
        }
        if let (Some(fixed), Some(tested)) = (self.resources.cpus, environment.cpu_count) {
            if fixed != tested {
                bail!("fixed CPU policy conflicts with controlled cpu_count");
            }
        }
        if let Some(fixed) = &self.resources.network {
            if environment.network_mode != "default" && &environment.network_mode != fixed {
                bail!("fixed network policy conflicts with controlled network mode");
            }
        }
        Ok(())
    }
    pub fn dispatch_build(&self) -> Result<()> {
        self.check()?;
        if let Some(credits) = &self.prepaid {
            let mut credits = credits
                .lock()
                .map_err(|_| anyhow::anyhow!("group lock poisoned"))?;
            if *credits == 0 {
                self.stop("budget_exhausted");
                bail!("confirmation group exhausted");
            }
            *credits -= 1;
            Ok(())
        } else {
            self.reserve(1)
        }
    }
    pub fn complete_group(&self, builds: u64) -> Result<Self> {
        // Cold ceiling: build + inspect/pull/reinspect + probe + three cleanup calls per container.
        // CID-file reconciliation only polls local state and does not launch more work.
        // Conservative reserved ceilings remain charged even if caches save work.
        let total = builds
            .checked_mul(11)
            .ok_or_else(|| anyhow::anyhow!("group allowance overflow"))?;
        let mut child = self.group(total)?;
        child.prepaid = Some(Arc::new(Mutex::new(builds)));
        child.prepaid_infrastructure = Some(Arc::new(Mutex::new(total - builds)));
        Ok(child)
    }
    fn reserve_infrastructure(&self, count: u64) -> Result<()> {
        self.check()?;
        if let Some(credits) = &self.prepaid_infrastructure {
            let mut credits = credits
                .lock()
                .map_err(|_| anyhow::anyhow!("infrastructure group lock poisoned"))?;
            if *credits < count {
                self.stop("budget_exhausted");
                bail!("confirmation infrastructure allowance exhausted");
            }
            *credits -= count;
            Ok(())
        } else {
            self.reserve(count)
        }
    }
    fn dispatch_infrastructure(&self) -> Result<()> {
        self.reserve_infrastructure(1)
    }

    pub fn group(&self, count: u64) -> Result<Self> {
        self.reserve(count)?;
        let mut child = self.clone();
        child.prepaid = Some(Arc::new(Mutex::new(count)));
        Ok(child)
    }
    pub fn operation_label(&self) -> String {
        format!("reprobisect.operation={}", self.id)
    }
    pub fn cancel(&self) {
        self.stop("cancelled");
    }
    pub fn stop(&self, reason: &'static str) {
        if let Ok(mut state) = self.state.lock() {
            state.reason.get_or_insert(reason);
        }
    }
    pub fn check(&self) -> Result<()> {
        self.reserve(0)
    }
    pub fn wait_child(
        &self,
        child: &mut std::process::Child,
        timeout: Duration,
    ) -> Result<std::process::ExitStatus> {
        let started = Instant::now();
        loop {
            if self.check().is_err() || started.elapsed() >= timeout {
                if self.reason().is_none() {
                    self.stop("attempt_timeout");
                }
                let _ = child.kill();
                let _ = child.wait();
                bail!("{}", self.reason().unwrap_or("operational_error"));
            }
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    /// Drain infrastructure output with a fixed retained bound, and never wait
    /// indefinitely on pipes retained by a descendant of the runtime client.
    pub fn output(&self, command: &mut std::process::Command) -> Result<std::process::Output> {
        self.output_on_spawn(command, || {})
    }
    pub fn output_on_spawn(
        &self,
        command: &mut std::process::Command,
        on_spawn: impl FnOnce(),
    ) -> Result<std::process::Output> {
        use std::{io::Read, process::Stdio, sync::mpsc};
        self.dispatch_infrastructure()?;
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .inspect_err(|_| self.stop("operational_error"))?;
        // A charged reservation is not evidence that a runtime request was sent.
        on_spawn();
        fn capture(
            mut pipe: impl Read + Send + 'static,
        ) -> mpsc::Receiver<std::io::Result<Vec<u8>>> {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let result = (|| {
                    let mut kept = Vec::new();
                    let mut buf = [0; 16384];
                    loop {
                        let n = pipe.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        let remaining = (1024_usize * 1024).saturating_sub(kept.len());
                        kept.extend_from_slice(&buf[..n.min(remaining)]);
                    }
                    Ok(kept)
                })();
                let _ = tx.send(result);
            });
            rx
        }
        let stdout = capture(
            child
                .stdout
                .take()
                .ok_or_else(|| anyhow::anyhow!("missing stdout"))?,
        );
        let stderr = capture(
            child
                .stderr
                .take()
                .ok_or_else(|| anyhow::anyhow!("missing stderr"))?,
        );
        let result = (|| -> Result<std::process::Output> {
            let status = self.wait_child(&mut child, Duration::from_secs(30))?;
            let stdout = stdout
                .recv_timeout(Duration::from_secs(1))
                .map_err(|_| anyhow::anyhow!("infrastructure stdout did not close"))??;
            let stderr = stderr
                .recv_timeout(Duration::from_secs(1))
                .map_err(|_| anyhow::anyhow!("infrastructure stderr did not close"))??;
            self.check()?;
            Ok(std::process::Output {
                status,
                stdout,
                stderr,
            })
        })();
        // Advisory consumers cannot turn an infrastructure interruption into
        // completed coverage. stop() preserves an earlier deadline/cancel/budget.
        result.inspect_err(|_| self.stop("operational_error"))
    }
    pub fn finish<T>(&self, project: &std::path::Path, result: Result<T>) -> Result<T> {
        use std::io::Write;
        let already_partial = self.reason().is_some();
        let result = match self.check() {
            Err(error) if !already_partial => Err(error),
            _ => result,
        };
        if result.is_err() {
            self.stop("operational_error");
        }
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("execution lock poisoned"))?;
        let receipt = serde_json::json!({
            "schema_version": 1, "operation_id": self.id,
            "charged_dispatches": state.charged, "maximum_dispatches": state.max,
            "elapsed_millis": self.started.elapsed().as_millis(),
            "completed": result.is_ok() && state.reason.is_none(),
            "completion_reason": state.reason.unwrap_or("completed"),
            "resource_policy": &*self.resources, "owned_containers": state.owned,
            "partial_attempts": state.partial_attempts,
            "accounting_policy": "conservative_pre_dispatch_and_complete_group_ceilings_no_refunds",
        });
        let persisted = (|| -> Result<()> {
            let directory = project.join(".reprobisect/executions");
            std::fs::create_dir_all(&directory)?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(format!("{}.json", self.id)))?;
            serde_json::to_writer_pretty(&mut file, &receipt)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            Ok(())
        })();
        // Release the state lock before stop() so a persistence failure is a
        // fail-closed infrastructure error, without overwriting a first reason.
        drop(state);
        persisted.inspect_err(|_| self.stop("operational_error"))?;
        result
    }
    pub fn persist_partial_attempt(
        &self,
        project: &std::path::Path,
        mut record: serde_json::Value,
    ) -> Result<()> {
        use std::io::Write;
        record["schema_version"] = 1.into();
        record["operation_id"] = self.id.to_string().into();
        record["completed"] = false.into();
        record["completion_reason"] = self.reason().unwrap_or("operational_error").into();
        let relative = format!(
            ".reprobisect/attempts/{}-{}.json",
            self.id,
            uuid::Uuid::new_v4()
        );
        std::fs::create_dir_all(project.join(".reprobisect/attempts"))?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(project.join(&relative))?;
        serde_json::to_writer_pretty(&mut file, &record)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("execution lock poisoned"))?
            .partial_attempts
            .push(relative);
        Ok(())
    }
    pub fn reason(&self) -> Option<&'static str> {
        self.state.lock().ok().and_then(|s| s.reason)
    }
}

/// Live bounded snapshots remain available even when a descendant holds a pipe.
#[derive(serde::Serialize)]
pub struct CaptureSnapshot {
    pub text: String,
    pub observed_sha256: String,
    pub observed_bytes: u64,
    pub retained_truncated: bool,
    pub complete: bool,
    pub error: Option<String>,
}
struct CaptureState {
    kept: Vec<u8>,
    hasher: sha2::Sha256,
    bytes: u64,
    closed: bool,
    error: Option<String>,
}
pub struct LiveCapture(Arc<Mutex<CaptureState>>, u64);
impl LiveCapture {
    pub fn start(mut pipe: impl std::io::Read + Send + 'static, limit: u64) -> Self {
        use sha2::Digest;
        let state = Arc::new(Mutex::new(CaptureState {
            kept: Vec::new(),
            hasher: sha2::Sha256::new(),
            bytes: 0,
            closed: false,
            error: None,
        }));
        let shared = state.clone();
        std::thread::spawn(move || {
            let mut buf = [0; 16384];
            loop {
                let read = pipe.read(&mut buf);
                let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
                match read {
                    Ok(0) => {
                        state.closed = true;
                        break;
                    }
                    Ok(n) => {
                        state.hasher.update(&buf[..n]);
                        state.bytes = state.bytes.saturating_add(n as u64);
                        let remaining = usize::try_from(limit)
                            .unwrap_or(usize::MAX)
                            .saturating_sub(state.kept.len());
                        state.kept.extend_from_slice(&buf[..n.min(remaining)]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        state.closed = true;
                        state.error = Some(e.to_string());
                        break;
                    }
                }
            }
        });
        Self(state, limit)
    }
    pub fn snapshot(&self) -> CaptureSnapshot {
        use sha2::Digest;
        let until = Instant::now() + Duration::from_secs(1);
        loop {
            let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if state.closed || Instant::now() >= until {
                return CaptureSnapshot {
                    text: String::from_utf8_lossy(&state.kept).into_owned(),
                    observed_sha256: hex::encode(state.hasher.clone().finalize()),
                    observed_bytes: state.bytes,
                    retained_truncated: state.bytes > self.1,
                    complete: state.closed && state.error.is_none(),
                    error: state.error.clone(),
                };
            }
            drop(state);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
/// Interrupted artifacts are bounded observations, not completed build artifacts.
/// No unbounded hashing or metadata parsing occurs in the safety grace.
pub fn interrupted_artifacts(
    workspace: &std::path::Path,
    outputs: &[std::path::PathBuf],
) -> serde_json::Value {
    use std::io::Read;
    let root = workspace.to_path_buf();
    let outputs = outputs.to_vec();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use sha2::Digest;
        let until = Instant::now() + Duration::from_secs(1);
        let mut observations = Vec::new();
        let mut retained = 0usize;
        let mut visited = 0usize;
        let mut truncated = false;
        let canonical_root = std::fs::canonicalize(&root).ok();
        'scan: for output in outputs {
            if output.is_absolute()
                || output
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                continue;
            }
            for entry in walkdir::WalkDir::new(root.join(output)).follow_links(false) {
                visited += 1;
                if visited > 128 || Instant::now() >= until || retained >= 1024 * 1024 {
                    truncated = true;
                    break 'scan;
                }
                let Ok(entry) = entry else {
                    continue;
                };
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                if !canonical_root
                    .as_ref()
                    .zip(std::fs::canonicalize(path).ok())
                    .is_some_and(|(root, path)| path.starts_with(root))
                {
                    continue;
                }
                let Ok(mut file) = std::fs::File::open(path) else {
                    continue;
                };
                let Ok(metadata) = file.metadata() else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                let mut bytes = Vec::new();
                if file
                    .by_ref()
                    .take((64 * 1024).min(1024 * 1024 - retained) as u64)
                    .read_to_end(&mut bytes)
                    .is_err()
                {
                    continue;
                }
                retained += bytes.len();
                observations.push(serde_json::json!({ "logical_path": path.strip_prefix(&root).ok(),
                    "size_bytes_observed": metadata.len(), "retained_bytes_hex": hex::encode(&bytes),
                    "sample_sha256": hex::encode(sha2::Sha256::digest(&bytes)),
                    "sample_covers_observed_size": bytes.len() as u64 == metadata.len(),
                    "artifact_completion": "unknown", "stable_snapshot": false }));
            }
        }
        let _ = tx.send(serde_json::json!({"observations": observations, "scan_truncated": truncated,
            "artifact_completion": "unknown", "qualification": "post_interrupt_bounded_observations_not_completed_build"}));
    });
    rx.recv_timeout(Duration::from_secs(1)).unwrap_or_else(|_| serde_json::json!({"observations": [], "scan_truncated": true, "artifact_completion": "unknown", "error": "artifact observation grace expired"}))
}

/// A cleanup capability requires a runtime-written CID acknowledgement outside
/// all mounted paths AND the matching operation label. Names are never removal targets.
pub struct OwnedContainer {
    name: String,
    runtime: &'static str,
    context: ExecutionContext,
    identity_dir: tempfile::TempDir,
    launched: bool,
    attempted: bool,
    verified: bool,
}
impl OwnedContainer {
    pub fn new(
        context: &ExecutionContext,
        backend: crate::model::RunnerBackend,
        purpose: &'static str,
    ) -> Result<Self> {
        context.reserve_infrastructure(3)?;
        let identity_dir = tempfile::Builder::new()
            .prefix("reprobisect-identity-")
            .tempdir()?;
        let name = format!("reprobisect-{purpose}-{}", uuid::Uuid::new_v4().simple());
        context
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("execution lock poisoned"))?
            .owned
            .push(OwnershipRecord {
                container_name: name.clone(),
                purpose,
                container_id: None,
                creation_outcome: "not_dispatched",
                cleanup_verified: false,
            });
        Ok(Self {
            name,
            runtime: backend.executable(),
            context: context.clone(),
            identity_dir,
            launched: false,
            attempted: false,
            verified: false,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn cidfile(&self) -> std::path::PathBuf {
        self.identity_dir.path().join("cid")
    }
    pub fn launched(&mut self) {
        self.launched = true;
    }
    fn record(&self, id: Option<String>, outcome: &'static str, verified: bool) {
        if let Ok(mut state) = self.context.state.lock() {
            if let Some(record) = state
                .owned
                .iter_mut()
                .find(|r| r.container_name == self.name)
            {
                record.container_id = id;
                record.creation_outcome = outcome;
                record.cleanup_verified = verified;
            }
        }
    }
    pub fn cleanup(&mut self) -> Result<()> {
        if self.attempted {
            anyhow::ensure!(self.verified, "owned cleanup previously failed");
            return Ok(());
        }
        self.attempted = true;
        if !self.launched {
            self.verified = true;
            self.record(None, "not_dispatched", true);
            return Ok(());
        }
        // A private finite grace, precharged to the operation, permits ONLY
        // identity reconciliation and exact-ID cleanup. An absent name is not
        // proof that an accepted daemon request will never finish creating.
        let mut cleanup = ExecutionContext::new(&crate::config::ExecutionConfig {
            max_dispatches: 3,
            total_timeout_seconds: 3,
            ..Default::default()
        })?;
        cleanup.ignore_signal = true;
        let id = loop {
            if let Ok(raw) = std::fs::read_to_string(self.cidfile()) {
                let id = raw.trim();
                if id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
                    break Some(id.to_string());
                }
            }
            if cleanup.check().is_err() {
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let Some(id) = id else {
            self.record(None, "unknown", false);
            self.context.stop("creation_unknown");
            bail!(
                "container creation outcome unknown; no acknowledged owned ID: {}",
                self.name
            );
        };
        self.record(Some(id.clone()), "acknowledged", false);
        let result = (|| -> Result<()> {
            let output = cleanup.output(std::process::Command::new(self.runtime).args([
                "container",
                "inspect",
                "--format",
                "{{json .}}",
                "--",
                &id,
            ]))?;
            anyhow::ensure!(output.status.success(), "owned ID inspect failed");
            let identity: serde_json::Value = serde_json::from_slice(&output.stdout)?;
            let operation = self.context.id.to_string();
            anyhow::ensure!(
                identity["Id"].as_str() == Some(&id)
                    && identity["Config"]["Labels"]["reprobisect.operation"].as_str()
                        == Some(&operation),
                "owned ID/operation identity mismatch"
            );
            let mut remove = std::process::Command::new(self.runtime);
            remove.args(["rm", "--force"]);
            if self.runtime == "podman" {
                // Podman force-rm otherwise waits its container stop timeout
                // (normally 10s), exceeding our private 3s safety grace.
                remove.args(["--time", "0"]);
            }
            let removed = cleanup.output(remove.args(["--", &id]))?;
            anyhow::ensure!(removed.status.success(), "exact owned ID removal failed");
            let absent = cleanup.output(std::process::Command::new(self.runtime).args([
                "ps",
                "--all",
                "--no-trunc",
                "--filter",
                &format!("id={id}"),
                "--format",
                "{{.ID}}",
            ]))?;
            anyhow::ensure!(
                absent.status.success() && absent.stdout.is_empty(),
                "owned ID absence unverified"
            );
            Ok(())
        })();
        self.verified = result.is_ok();
        self.record(Some(id), "acknowledged", self.verified);
        if result.is_err() {
            self.context.stop("cleanup_failed");
        }
        result
    }
}
impl Drop for OwnedContainer {
    fn drop(&mut self) {
        if !self.attempted {
            let _ = self.cleanup();
        }
    }
}

pub fn completion_reason(notes: &[String]) -> Option<&'static str> {
    [
        "budget_exhausted",
        "deadline_exhausted",
        "cancelled",
        "attempt_timeout",
        "cleanup_failed",
        "creation_unknown",
        "operational_error",
    ]
    .into_iter()
    .find(|reason| {
        notes
            .iter()
            .any(|n| n == &format!("execution_completion={reason}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn earlier_stop_reason_survives_refused_output_and_receipt_io_failure() {
        for reason in [
            "deadline_exhausted",
            "cancelled",
            "budget_exhausted",
            "attempt_timeout",
            "cleanup_failed",
            "creation_unknown",
        ] {
            let project = tempfile::tempdir().unwrap();
            std::fs::create_dir(project.path().join(".reprobisect")).unwrap();
            std::fs::write(
                project.path().join(".reprobisect/executions"),
                b"obstruction",
            )
            .unwrap();
            let context =
                ExecutionContext::new(&crate::config::ExecutionConfig::default()).unwrap();
            context.stop(reason);
            let mut launched = false;
            assert!(
                context
                    .output_on_spawn(
                        &mut std::process::Command::new("definitely-not-a-runtime"),
                        || launched = true
                    )
                    .is_err()
            );
            assert!(!launched);
            assert_eq!(context.state.lock().unwrap().charged, 0);
            assert!(
                context
                    .finish(
                        project.path(),
                        Err::<(), _>(anyhow::anyhow!("later operational failure"))
                    )
                    .is_err()
            );
            assert_eq!(context.reason(), Some(reason));
        }
    }

    #[test]
    fn completion_cannot_turn_an_expired_context_into_success() {
        let root = tempfile::tempdir().unwrap();
        let context = ExecutionContext::new(&crate::config::ExecutionConfig {
            total_timeout_seconds: 0,
            ..Default::default()
        })
        .unwrap();
        assert!(context.finish(root.path(), Ok(())).is_err());
        let path = std::fs::read_dir(root.path().join(".reprobisect/executions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(receipt["completed"], false);
        assert_eq!(receipt["completion_reason"], "deadline_exhausted");
    }

    #[test]
    fn owned_cleanup_capacity_is_reserved_before_container_can_launch() {
        let context = ExecutionContext::new(&crate::config::ExecutionConfig {
            max_dispatches: 1,
            ..Default::default()
        })
        .unwrap();
        assert!(
            OwnedContainer::new(&context, crate::model::RunnerBackend::Docker, "probe").is_err()
        );
        assert_eq!(context.reason(), Some("budget_exhausted"));
        assert_eq!(context.state.lock().unwrap().charged, 0);
    }

    #[test]
    fn restricted_profile_has_explicit_finite_resource_policy() {
        let context = ExecutionContext::new(&crate::config::ExecutionConfig {
            resources: crate::config::ResourcePolicy {
                restricted: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
        assert_eq!(context.resources.cpus, Some(2));
        assert_eq!(context.resources.pids, Some(256));
        assert_eq!(context.resources.memory_bytes, Some(1_073_741_824));
        assert_eq!(context.resources.network.as_deref(), Some("none"));
    }

    #[test]
    fn complete_confirmation_group_includes_cold_infrastructure() {
        let context = ExecutionContext::new(&crate::config::ExecutionConfig {
            max_dispatches: 4,
            ..Default::default()
        })
        .unwrap();
        assert!(
            context.complete_group(1).is_err(),
            "one cold confirmation needs build + inspect/pull/reinspect/probe capacity"
        );
        assert_eq!(context.state.lock().unwrap().charged, 0);
    }

    #[test]
    fn reserved_builds_cannot_double_charge_or_escape_the_group() {
        let context = ExecutionContext::new(&crate::config::ExecutionConfig {
            max_dispatches: 2,
            ..Default::default()
        })
        .unwrap();
        let group = context.group(2).unwrap();
        group.dispatch_build().expect("first prepaid build");
        group
            .clone()
            .dispatch_build()
            .expect("second prepaid build");
        assert_eq!(context.state.lock().unwrap().charged, 2);
        assert!(group.dispatch_build().is_err());
    }

    #[test]
    fn cumulative_confirmation_group_is_reserved_atomically() {
        let context = ExecutionContext::new(&crate::config::ExecutionConfig {
            max_dispatches: 4,
            ..Default::default()
        })
        .unwrap();
        context.reserve(2).unwrap();
        assert!(
            context.group(3).is_err(),
            "cannot begin only part of a confirmation group"
        );
        assert_eq!(context.state.lock().unwrap().charged, 2);
    }

    #[test]
    fn total_deadline_includes_time_before_first_dispatch() {
        let config = toml::from_str::<crate::config::Config>(
            r#"
[build]
image = "unused"
command = ["true"]
outputs = ["out"]
[execution]
total_timeout_seconds = 0
"#,
        )
        .unwrap();
        let context = ExecutionContext::new(&config.execution).unwrap();
        assert!(
            context.reserve(1).is_err(),
            "expired operation must not dispatch"
        );
        assert_eq!(context.reason(), Some("deadline_exhausted"));
    }
}
