use std::process::{Command, Stdio};

// A real CLI subprocess and runtime-client executable; never runtime qualification.
fn runtime_double() -> &'static std::path::Path {
    static DOUBLE: std::sync::OnceLock<(tempfile::TempDir, std::path::PathBuf)> =
        std::sync::OnceLock::new();
    DOUBLE
        .get_or_init(|| {
            let dir = tempfile::tempdir().unwrap();
            let executable = dir
                .path()
                .join(format!("docker{}", std::env::consts::EXE_SUFFIX));
            let mut compiler = Command::new("rustc");
            compiler.args(["--edition", "2024"]);
            if let Ok(flags) = std::env::var("RUSTFLAGS") {
                compiler.args(flags.split_whitespace());
            }
            let output = compiler
                .arg(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/support/runtime-double.rs"),
                )
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            std::fs::copy(
                &executable,
                dir.path()
                    .join(format!("podman{}", std::env::consts::EXE_SUFFIX)),
            )
            .unwrap();
            (dir, executable)
        })
        .0
        .path()
}
fn double_command(project: &std::path::Path, mode: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_reprobisect"));
    let path = std::env::join_paths(
        std::iter::once(runtime_double().to_path_buf())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    command
        .env("PATH", path)
        .env("PROBE_ROOT", project)
        .env("PROBE_MODE", mode);
    command
}
fn double_check(mode: &str) -> (tempfile::TempDir, std::process::Output, serde_json::Value) {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\ntimeout_seconds=1\n[execution]\ntotal_timeout_seconds=30\n").unwrap();
    let output = double_command(project.path(), mode)
        .args(["check", "--ci", "--ci-policy", "report-only"])
        .arg(project.path())
        .output()
        .unwrap();
    let receipt_path = std::fs::read_dir(project.path().join(".reprobisect/executions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let receipt = serde_json::from_slice(&std::fs::read(receipt_path).unwrap()).unwrap();
    (project, output, receipt)
}
fn execution_receipt(project: &std::path::Path) -> serde_json::Value {
    let path = std::fs::read_dir(project.join(".reprobisect/executions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn execution_failure_details(
    phase: &str,
    receipt: &serde_json::Value,
    output: &std::process::Output,
) -> String {
    let bounded =
        |bytes: &[u8]| String::from_utf8_lossy(&bytes[..bytes.len().min(8192)]).into_owned();
    format!(
        "phase={phase}; receipt={}; exit={:?}; stdout={}; stderr={}",
        bounded(receipt.to_string().as_bytes()),
        output.status.code(),
        bounded(&output.stdout),
        bounded(&output.stderr)
    )
}

#[test]
fn public_successful_build_parent_with_open_pipes_is_operational_error() {
    for route in ["check", "diagnose"] {
        for policy in [Some("require-reproducible"), Some("report-only"), None] {
            let project = tempfile::tempdir().unwrap();
            std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\ntimeout_seconds=20\n[execution]\ntotal_timeout_seconds=60\n").unwrap();
            let mut command = double_command(project.path(), "successful-parent-pipes");
            command.arg(route);
            if let Some(policy) = policy {
                command.args(["--ci", "--ci-policy", policy]);
            }
            let output = command.arg(project.path()).output().unwrap();
            let receipt = execution_receipt(project.path());
            assert_eq!(receipt["completion_reason"], "operational_error");
            assert_eq!(receipt["completed"], false);
            let attempt: serde_json::Value = serde_json::from_slice(
                &std::fs::read(
                    project
                        .path()
                        .join(receipt["partial_attempts"][0].as_str().unwrap()),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(
                attempt["exit_status"], 0,
                "must test successful parent, not timeout"
            );
            assert_eq!(attempt["stdout"]["complete"], false);
            assert!(
                attempt["artifacts"]["observations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v["logical_path"] == "out")
            );
            assert_eq!(
                output.status.code(),
                Some(5),
                "{route}/{policy:?}: {output:?}"
            );
            if policy.is_some() {
                let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(envelope["completion"]["reason"], "operational_error");
                assert_eq!(envelope["completion"]["completed"], false);
                assert_eq!(envelope["coverage"]["complete"], false);
                assert_eq!(envelope["policy"]["passed"], false);
            }
        }
    }
}

#[test]
fn public_probe_predispatch_refusal_is_known_not_dispatched() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[execution]\nmax_dispatches=7\ntotal_timeout_seconds=30\n").unwrap();
    let output = double_command(project.path(), "stable")
        .args(["check", "--ci", "--ci-policy", "report-only"])
        .arg(project.path())
        .output()
        .unwrap();
    let receipt = execution_receipt(project.path());
    assert_eq!(receipt["charged_dispatches"], 7);
    assert_eq!(receipt["completion_reason"], "budget_exhausted");
    let log = std::fs::read_to_string(project.path().join("runtime.log")).unwrap();
    assert_eq!(log.lines().count(), 2, "only info and image inspect: {log}");
    assert!(log.lines().all(|l| !l.starts_with("run |")));
    let owned = receipt["owned_containers"].as_array().unwrap();
    assert_eq!(owned.len(), 1);
    assert_eq!(owned[0]["purpose"], "probe");
    assert_eq!(owned[0]["creation_outcome"], "not_dispatched", "{receipt}");
    assert_eq!(owned[0]["cleanup_verified"], true);
    assert!(owned[0]["container_id"].is_null());
    assert!(
        receipt["elapsed_millis"].as_u64().unwrap() < 2500,
        "no CID reconciliation for refused spawn: {receipt}"
    );
    assert_eq!(output.status.code(), Some(5));
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["completion"]["reason"], "operational_error");
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            project
                .path()
                .join(envelope["report"]["path"].as_str().unwrap()),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        report["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "execution_completion=budget_exhausted")
    );
    assert_eq!(envelope["policy"]["passed"], false);
}

#[test]
fn public_probe_failed_spawn_is_known_not_dispatched_and_stops_work() {
    let project = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    let fallback = tempfile::tempdir().unwrap();
    let executable = format!("docker{}", std::env::consts::EXE_SUFFIX);
    for dir in [runtime.path(), fallback.path()] {
        std::fs::copy(runtime_double().join(&executable), dir.join(&executable)).unwrap();
    }
    // A later executable makes PATH fallthrough observable even without system Docker.
    // Keep inherited PATH (and Git provenance) so this still charges exactly eight.
    let path = std::env::join_paths(
        [runtime.path().to_path_buf(), fallback.path().to_path_buf()]
            .into_iter()
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[execution]\ntotal_timeout_seconds=30\n").unwrap();
    let output = double_command(project.path(), "failed-probe-spawn")
        .env("PATH", &path)
        .args(["check", "--ci", "--ci-policy", "report-only"])
        .arg(project.path())
        .output()
        .unwrap();
    assert!(
        project.path().join("retired-runtime.exe").is_file(),
        "double must displace executable after image inspect"
    );
    let receipt = execution_receipt(project.path());
    let log = std::fs::read_to_string(project.path().join("runtime.log")).unwrap();
    let details = execution_failure_details("failed-spawn", &receipt, &output);
    assert_eq!(
        log.lines().count(),
        2,
        "only info and image inspect: {log}; {details}"
    );
    let owned = receipt["owned_containers"].as_array().unwrap();
    assert_eq!(owned[0]["creation_outcome"], "not_dispatched", "{details}");
    assert_eq!(owned[0]["cleanup_verified"], true, "{details}");
    assert_eq!(
        owned.len(),
        1,
        "no additional reservations after infrastructure spawn failed; {details}"
    );
    assert_eq!(
        receipt["charged_dispatches"], 8,
        "git provenance + version + build + image + 3 cleanup + failed spawn remain charged, no refunds; {details}"
    );
    assert_eq!(
        receipt["completion_reason"], "operational_error",
        "{details}"
    );
    assert!(
        receipt["elapsed_millis"].as_u64().unwrap() < 2500,
        "no CID reconciliation for failed spawn; {details}"
    );
    assert_eq!(output.status.code(), Some(5), "{details}");
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["completion"]["reason"], "operational_error");
    assert_eq!(envelope["policy"]["passed"], false);
    let error = Command::new("docker")
        .env("PATH", &path)
        .env("PROBE_ROOT", project.path())
        .env("PROBE_MODE", "failed-probe-spawn")
        .arg("version")
        .spawn()
        .expect_err("fixture must fail at OS spawn, not run a later runtime or shell");
    #[cfg(target_os = "linux")]
    assert_eq!(
        error.raw_os_error(),
        Some(40),
        "ELOOP, not PATH-search EACCES/ENOEXEC: {error}"
    );
    #[cfg(not(target_os = "linux"))]
    let _ = error;
    assert_eq!(
        std::fs::read_to_string(project.path().join("runtime.log")).unwrap(),
        log
    );
}

#[test]
fn public_toolchain_probe_open_streams_cannot_report_success() {
    for route in ["check", "diagnose"] {
        for policy in [Some("require-reproducible"), Some("report-only"), None] {
            let project = tempfile::tempdir().unwrap();
            std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\ntimeout_seconds=20\n[experiments.dimensions]\nbuild_path=false\nsource_date_epoch=false\ntimezone=false\nlocale=false\nhostname=false\n[execution]\ntotal_timeout_seconds=60\n").unwrap();
            let mut command = double_command(project.path(), "probe-open-pipes");
            command.arg(route);
            if let Some(policy) = policy {
                command.args(["--ci", "--ci-policy", policy]);
            }
            let output = command.arg(project.path()).output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(5),
                "{route}/{policy:?}: {output:?}"
            );
            let receipt = execution_receipt(project.path());
            assert_eq!(receipt["completion_reason"], "operational_error");
            assert_eq!(receipt["completed"], false);
            assert_eq!(receipt["charged_dispatches"], 8);
            let owned = receipt["owned_containers"].as_array().unwrap();
            assert_eq!(
                owned.len(),
                1,
                "must stop shared context before build reservations"
            );
            assert_eq!(owned[0]["purpose"], "probe");
            assert_eq!(owned[0]["creation_outcome"], "acknowledged");
            assert_eq!(owned[0]["cleanup_verified"], true);
            let log = std::fs::read_to_string(project.path().join("runtime.log")).unwrap();
            assert_eq!(log.lines().filter(|l| l.starts_with("run |")).count(), 1);
            assert!(!project.path().join("build-name").exists());
            if policy.is_some() {
                let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(envelope["completion"]["reason"], "operational_error");
                assert_eq!(envelope["completion"]["completed"], false);
                assert_eq!(envelope["coverage"]["complete"], false);
                assert_eq!(envelope["policy"]["passed"], false);
                let report: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(
                        project
                            .path()
                            .join(envelope["report"]["path"].as_str().unwrap()),
                    )
                    .unwrap(),
                )
                .unwrap();
                assert_eq!(report["status"], "inconclusive");
                assert!(report["runs"].as_array().unwrap().is_empty());
            }
        }
    }
}

#[test]
fn public_podman_interrupted_cleanup_is_exact_and_bounded() {
    for backend in ["podman", "docker"] {
        let project = tempfile::tempdir().unwrap();
        let config = format!(
            "[build]\nrunner='{backend}'\nimage='unused'\ncommand=['true']\noutputs=['out']\ntimeout_seconds=1\n[execution]\ntotal_timeout_seconds=30\n"
        );
        std::fs::write(project.path().join(".reprobisect.toml"), &config).unwrap();
        let unrelated_id = "b".repeat(64);
        std::fs::write(project.path().join(&unrelated_id), b"unrelated\nother").unwrap();
        let output = double_command(project.path(), "podman-stop-grace")
            .args(["check", "--ci", "--ci-policy", "report-only"])
            .arg(project.path())
            .output()
            .unwrap();
        let receipt = execution_receipt(project.path());
        let details = execution_failure_details(backend, &receipt, &output);
        assert_eq!(output.status.code(), Some(5), "{details}");
        assert_eq!(receipt["completion_reason"], "attempt_timeout", "{details}");
        assert_eq!(receipt["completed"], false, "{details}");
        assert_eq!(
            receipt["charged_dispatches"], 11,
            "no extra cleanup commands or refunds; {details}"
        );
        let owned = receipt["owned_containers"].as_array().unwrap();
        assert_eq!(
            owned.len(),
            2,
            "one probe and one interrupted build; {details}"
        );
        assert!(
            owned.iter().all(|c| c["cleanup_verified"] == true),
            "{details}"
        );
        assert_eq!(owned[0]["purpose"], "probe", "{details}");
        assert_eq!(owned[1]["purpose"], "build", "{details}");
        let log = std::fs::read_to_string(project.path().join("runtime.log")).unwrap();
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(
            lines.len(),
            10,
            "only cold preparation, probe and build with three cleanup calls each: {log}; {details}"
        );
        assert_eq!(
            lines.iter().filter(|l| l.starts_with("run | ")).count(),
            2,
            "{log}"
        );
        for container in owned {
            assert_eq!(container["creation_outcome"], "acknowledged", "{details}");
            let id = container["container_id"].as_str().unwrap();
            assert_eq!(id.len(), 64, "{details}");
            let inspect = format!("container | inspect | --format | {{{{json .}}}} | -- | {id}");
            let remove = if backend == "podman" {
                format!("rm | --force | --time | 0 | -- | {id}")
            } else {
                format!("rm | --force | -- | {id}")
            };
            let absent =
                format!("ps | --all | --no-trunc | --filter | id={id} | --format | {{{{.ID}}}}");
            let index = lines.iter().position(|l| l == &inspect).unwrap();
            assert_eq!(lines[index + 1], remove, "{log}; {details}");
            assert_eq!(lines[index + 2], absent, "{log}; {details}");
            assert!(!project.path().join(id).exists(), "{details}");
        }
        assert!(
            !project.path().join("default-stop-grace").exists(),
            "{details}"
        );
        assert_eq!(
            std::fs::read(project.path().join(&unrelated_id)).unwrap(),
            b"unrelated\nother"
        );
        assert_eq!(
            std::fs::read_to_string(project.path().join(".reprobisect.toml")).unwrap(),
            config
        );
        let attempts = receipt["partial_attempts"].as_array().unwrap();
        assert_eq!(attempts.len(), 1, "{details}");
        let attempt: serde_json::Value = serde_json::from_slice(
            &std::fs::read(project.path().join(attempts[0].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        assert_eq!(
            attempt["runner_backend"], backend,
            "actual public backend; {details}"
        );
        assert_eq!(attempt["cleanup_verified"], true, "{details}");
        assert_eq!(attempt["completed"], false);
        assert!(
            attempt["stdout"]["text"]
                .as_str()
                .unwrap()
                .contains("actual stdout before interrupt")
        );
        assert!(
            attempt["artifacts"]["observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["logical_path"] == "out"
                    && v["retained_bytes_hex"] == hex::encode(b"actual interrupted artifact"))
        );
        assert!(!project.path().join("out").exists());
        let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(envelope["completion"]["reason"], "operational_error");
        assert_eq!(envelope["completion"]["completed"], false);
        assert_eq!(envelope["coverage"]["complete"], false);
        assert_eq!(envelope["policy"]["passed"], false);
        let report: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                project
                    .path()
                    .join(envelope["report"]["path"].as_str().unwrap()),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(report["status"], "inconclusive");
        assert!(report["runs"].as_array().unwrap().is_empty());
    }
}

#[test]
fn runtime_interrupt_retains_active_capture_and_artifact_without_completed_run() {
    for mode in ["timeout", "pipes"] {
        let (project, output, receipt) = double_check(mode);
        assert_eq!(output.status.code(), Some(5));
        let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(envelope["policy"]["passed"], false);
        let report: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                project
                    .path()
                    .join(envelope["report"]["path"].as_str().unwrap()),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(report["runs"].as_array().unwrap().is_empty());
        let attempts = receipt["partial_attempts"]
            .as_array()
            .expect("interrupted attempt records required");
        assert_eq!(attempts.len(), 1);
        let attempt: serde_json::Value = serde_json::from_slice(
            &std::fs::read(project.path().join(attempts[0].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        assert_eq!(attempt["completed"], false);
        assert!(
            attempt["stdout"]["text"]
                .as_str()
                .unwrap()
                .contains("actual stdout before interrupt")
        );
        assert_eq!(attempt["stdout"]["complete"], mode != "pipes");
        assert_eq!(
            attempt["artifacts"]["observations"][0]["logical_path"],
            "out"
        );
        assert_eq!(
            attempt["artifacts"]["observations"][0]["retained_bytes_hex"],
            hex::encode(b"actual interrupted artifact")
        );
        assert!(!project.path().join("out").exists());
    }
}

#[test]
fn cumulative_baseline_deadline_retains_first_complete_run_and_second_attempt() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\ntimeout_seconds=20\n[execution]\ntotal_timeout_seconds=4\n").unwrap();
    let output = double_command(project.path(), "baseline-late")
        .args(["check", "--ci", "--ci-policy", "report-only"])
        .arg(project.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5), "{output:?}");
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["policy"]["passed"], false);
    assert_eq!(envelope["coverage"]["complete"], false);
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            project
                .path()
                .join(envelope["report"]["path"].as_str().unwrap()),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(report["runs"].as_array().unwrap().len(), 1);
    assert_eq!(report["status"], "inconclusive");
    let path = std::fs::read_dir(project.path().join(".reprobisect/executions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(receipt["completion_reason"], "deadline_exhausted");
    assert_eq!(receipt["partial_attempts"].as_array().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(project.path().join("build-count")).unwrap(),
        "2"
    );
    assert!(receipt["charged_dispatches"].as_u64().unwrap() >= 15);
}

#[test]
fn ci_public_effective_restricted_defaults_have_same_identity_and_runtime_policy() {
    let mut identities = Vec::new();
    let mut policies = Vec::new();
    for explicit in [false, true] {
        let project = tempfile::tempdir().unwrap();
        let config = format!(
            "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[experiments.dimensions]\nbuild_path=false\nsource_date_epoch=false\ntimezone=false\nlocale=false\nhostname=false\n[execution.resources]\nrestricted=true\n{}",
            if explicit {
                "cpus=2\nmemory_bytes=1073741824\npids=256\nnetwork='none'\n"
            } else {
                ""
            }
        );
        std::fs::write(project.path().join(".reprobisect.toml"), config).unwrap();
        let output = double_command(project.path(), "stable")
            .args(["check", "--ci"])
            .arg(project.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(envelope["completion"]["completed"], true);
        identities.push(envelope["identities"]["effective_config_sha256"].clone());
        let log = std::fs::read_to_string(project.path().join("runtime.log")).unwrap();
        let launches: Vec<_> = log
            .lines()
            .filter(|l| l.starts_with("run |") && l.contains("reprobisect-build-"))
            .collect();
        assert_eq!(launches.len(), 2);
        for line in launches {
            for flag in [
                "--cpus | 2",
                "--memory | 1073741824",
                "--pids-limit | 256",
                "--network | none",
            ] {
                assert!(line.contains(flag), "{line}");
            }
        }
        let path = std::fs::read_dir(project.path().join(".reprobisect/executions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        policies.push(receipt["resource_policy"].clone());
    }
    assert!(identities[0].is_string());
    assert_eq!(identities[0], identities[1]);
    assert_eq!(policies[0], policies[1]);
}

#[test]
fn zero_deadline_skips_substantial_source_preparation_without_source_identity() {
    let project = tempfile::tempdir().unwrap();
    let source = project.path().join("substantial-source");
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    let file = options.open(&source).unwrap();
    file.set_len(128 * 1024 * 1024).unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[execution]\ntotal_timeout_seconds=0\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .args(["check", "--ci"])
        .arg(project.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("failed to digest source tree"),
        "expired operation entered source hashing: {output:?}"
    );
    assert!(envelope["identities"]["source_sha256"].is_null());
    let receipt_path = std::fs::read_dir(project.path().join(".reprobisect/executions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(receipt_path).unwrap()).unwrap();
    assert_eq!(
        receipt["completion_reason"], "deadline_exhausted",
        "{output:?}"
    );
    assert_eq!(receipt["charged_dispatches"], 0);
    assert!(receipt["owned_containers"].as_array().unwrap().is_empty());
    assert!(
        receipt["elapsed_millis"].as_u64().unwrap() < 500,
        "expired operation must not hash 128 MiB"
    );
    drop(file);
}

#[test]
fn public_init_missing_runtime_is_exit_five_without_success_config() {
    let project = tempfile::tempdir().unwrap();
    let empty_path = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("Makefile"), "all:\n\ttrue\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .env("PATH", empty_path.path())
        .args(["init", "--discover-outputs"])
        .arg(project.path())
        .output()
        .unwrap();
    let receipt = execution_receipt(project.path());
    assert_eq!(receipt["completion_reason"], "operational_error");
    assert_eq!(receipt["completed"], false);
    assert_eq!(receipt["charged_dispatches"], 4);
    assert_eq!(
        receipt["owned_containers"][0]["creation_outcome"],
        "not_dispatched"
    );
    assert_eq!(receipt["owned_containers"][0]["cleanup_verified"], true);
    assert_eq!(output.status.code(), Some(5), "{output:?}");
    assert!(!project.path().join(".reprobisect.toml").exists());
}

#[test]
fn public_init_receipt_io_failure_is_exit_five_without_success_config() {
    for mode in ["stable", "ordinary-build-failed"] {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("Makefile"), "all:\n\ttrue\n").unwrap();
        std::fs::create_dir(project.path().join(".reprobisect")).unwrap();
        let obstruction = project.path().join(".reprobisect/executions");
        std::fs::write(&obstruction, b"receipt directory obstruction").unwrap();
        let output = double_command(project.path(), mode)
            .args(["init", "--discover-outputs"])
            .arg(project.path())
            .output()
            .unwrap();
        assert_eq!(
            std::fs::read(&obstruction).unwrap(),
            b"receipt directory obstruction"
        );
        let log = std::fs::read_to_string(project.path().join("runtime.log")).unwrap();
        assert_eq!(log.lines().filter(|l| l.starts_with("run |")).count(), 1);
        assert_eq!(log.lines().filter(|l| l.starts_with("rm |")).count(), 1);
        assert_eq!(log.lines().filter(|l| l.starts_with("ps |")).count(), 1);
        assert_eq!(output.status.code(), Some(5), "{mode}: {output:?}");
        assert!(!project.path().join(".reprobisect.toml").exists());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("created "));
        assert!(String::from_utf8_lossy(&output.stderr).contains("output discovery interrupted"));
    }
}

#[test]
fn public_init_executed_failed_build_remains_advisory() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("Makefile"), "all:\n\tfalse\n").unwrap();
    let output = double_command(project.path(), "ordinary-build-failed")
        .args(["init", "--discover-outputs"])
        .arg(project.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let config = std::fs::read_to_string(project.path().join(".reprobisect.toml")).unwrap();
    assert!(config.contains("kept static output inference"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("temporary discovery build failed"));
    let receipt = execution_receipt(project.path());
    assert_eq!(receipt["completed"], false);
    assert_eq!(
        receipt["owned_containers"][0]["creation_outcome"],
        "acknowledged"
    );
    assert_eq!(receipt["owned_containers"][0]["cleanup_verified"], true);
    assert!(receipt["partial_attempts"].as_array().unwrap().is_empty());
    let log = std::fs::read_to_string(project.path().join("runtime.log")).unwrap();
    assert_eq!(log.lines().filter(|l| l.starts_with("run |")).count(), 1);
}

#[test]
fn init_discovery_cleanup_failure_is_exit_five_without_success_config() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("Makefile"), "all:\n\ttrue\n").unwrap();
    let output = double_command(project.path(), "cleanupfail")
        .args(["init", "--discover-outputs"])
        .arg(project.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5), "{output:?}");
    assert!(!project.path().join(".reprobisect.toml").exists());
    let path = std::fs::read_dir(project.path().join(".reprobisect/executions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(receipt["completed"], false);
    assert_eq!(receipt["completion_reason"], "cleanup_failed");
    let attempt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            project
                .path()
                .join(receipt["partial_attempts"][0].as_str().unwrap()),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(attempt["purpose"], "discovery");
    assert_eq!(attempt["completed"], false);
    assert!(
        attempt["stdout"]["text"]
            .as_str()
            .unwrap()
            .contains("actual stdout before interrupt")
    );
    assert!(
        attempt["artifacts"]["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["logical_path"] == "out")
    );
}

#[test]
#[cfg(target_os = "linux")]
fn init_discovery_cancellation_public_route_is_exit_five_and_retains_active_evidence() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("Makefile"), "all:\n\ttrue\n").unwrap();
    let child = double_command(project.path(), "timeout")
        .args(["init", "--discover-outputs"])
        .arg(project.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !project.path().join("build-name").exists() {
        assert!(
            std::time::Instant::now() < until,
            "discovery launch was not observed"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let output = bounded_wait(child);
    assert_eq!(output.status.code(), Some(5), "{output:?}");
    assert!(!project.path().join(".reprobisect.toml").exists());
    let path = std::fs::read_dir(project.path().join(".reprobisect/executions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(receipt["completion_reason"], "cancelled");
    assert_eq!(receipt["completed"], false);
    let attempt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            project
                .path()
                .join(receipt["partial_attempts"][0].as_str().unwrap()),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        attempt["stdout"]["text"]
            .as_str()
            .unwrap()
            .contains("actual stdout before interrupt")
    );
    assert!(
        attempt["artifacts"]["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["logical_path"] == "out")
    );
}

#[test]
fn runtime_collision_survives_without_owned_identity() {
    let (project, output, receipt) = double_check("collision");
    assert_eq!(output.status.code(), Some(5));
    assert!(project.path().join("unrelated-container").exists());
    let build = receipt["owned_containers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["purpose"] == "build")
        .unwrap();
    assert_eq!(build["cleanup_verified"], false);
    assert_eq!(build["creation_outcome"], "unknown");
    let log = std::fs::read_to_string(project.path().join("runtime.log")).unwrap();
    for line in log.lines().filter(|l| l.starts_with("rm |")) {
        assert_eq!(line.split(" | ").last().unwrap().len(), 64);
    }
}
#[test]
fn runtime_late_unacknowledged_creation_never_claims_cleanup() {
    let (project, output, receipt) = double_check("race");
    assert_eq!(output.status.code(), Some(5));
    let build = receipt["owned_containers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["purpose"] == "build")
        .unwrap();
    assert_eq!(build["cleanup_verified"], false);
    assert_eq!(build["creation_outcome"], "unknown");
    assert!(build["container_id"].is_null());
    assert_eq!(receipt["charged_dispatches"], 11);
    std::thread::sleep(std::time::Duration::from_millis(2500));
    assert!(project.path().join("late-owned-container").exists());
}
#[test]
fn runtime_delayed_ack_cleans_exact_id_without_deleting_reused_name() {
    let (project, output, receipt) = double_check("ack-late");
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(receipt["completed"], false);
    assert!(
        receipt["owned_containers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["cleanup_verified"] == true)
    );
    assert!(!project.path().join("late-owned-container").exists());
    assert!(project.path().join("reused-name-unrelated").exists());
}
#[test]
fn runtime_wrong_operation_identity_is_never_removed() {
    let (project, output, receipt) = double_check("identity-mismatch");
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(receipt["owned_containers"][0]["cleanup_verified"], false);
    assert!(
        !std::fs::read_to_string(project.path().join("runtime.log"))
            .unwrap()
            .lines()
            .any(|l| l.starts_with("rm |"))
    );
}

#[test]
fn ci_read_only_stderr_cannot_suppress_stdout() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "not valid TOML").unwrap();
    let diagnostic_path = project.path().join("stderr");
    std::fs::write(&diagnostic_path, b"untouched").unwrap();
    let mut codes = vec![];
    for (args, expected, reason) in [
        (vec!["check", "--ci", "--not-a-flag"], 2, "usage_error"),
        (
            vec!["diagnose", "--ci", "missing-project"],
            5,
            "operational_error",
        ),
        (vec!["check", "--ci"], 5, "operational_error"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
            .current_dir(project.path())
            .args(args)
            .stderr(Stdio::from(std::fs::File::open(&diagnostic_path).unwrap()))
            .output()
            .unwrap();
        codes.push((output.status.code(), Some(expected)));
        if output.status.code() == Some(expected) {
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["completion"]["reason"], reason);
            assert_eq!(value["policy"]["exit_code"], expected);
            assert!(!value["policy"]["passed"].as_bool().unwrap());
        } else {
            assert!(
                output.stdout.is_empty(),
                "unexpected failing-process output"
            );
        }
    }
    assert_eq!(std::fs::read(&diagnostic_path).unwrap(), b"untouched");
    assert!(
        codes.iter().all(|(actual, expected)| actual == expected),
        "exit codes: {codes:?}"
    );
}

#[test]
fn ci_unwritable_stdout_is_operational_even_for_usage_errors() {
    let project = tempfile::tempdir().unwrap();
    let path = project.path().join("stdout");
    std::fs::write(&path, b"untouched").unwrap();
    for args in [
        vec!["check", "--ci", "--not-a-flag"],
        vec!["diagnose", "--ci", "missing-project"],
    ] {
        for read_only_stderr in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_reprobisect"));
            command
                .current_dir(project.path())
                .args(&args)
                .stdout(Stdio::from(std::fs::File::open(&path).unwrap()));
            if read_only_stderr {
                command.stderr(Stdio::from(std::fs::File::open(&path).unwrap()));
            }
            let output = command.output().unwrap();
            assert_eq!(output.status.code(), Some(5), "{output:?}");
            assert_eq!(std::fs::read(&path).unwrap(), b"untouched");
        }
    }
}

#[test]
fn budget_exhaustion_never_reports_reproducible() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join(".reprobisect.toml"),
        r#"
[build]
image = "not-needed:local"
command = ["true"]
outputs = ["out"]
[execution]
max_dispatches = 0
"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .args(["check", "--ci", "--ci-policy", "report-only"])
        .arg(project.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["completion"]["reason"], "operational_error");
    assert_eq!(value["diagnostic_status"], "inconclusive");
    assert_eq!(value["coverage"]["complete"], false);
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            project
                .path()
                .join(value["report"]["path"].as_str().unwrap()),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(report["status"], "inconclusive");
    assert_eq!(report["runs"].as_array().unwrap().len(), 0);
    let receipts: Vec<_> = std::fs::read_dir(project.path().join(".reprobisect/executions"))
        .unwrap()
        .collect();
    assert_eq!(receipts.len(), 1);
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(receipts[0].as_ref().unwrap().path()).unwrap())
            .unwrap();
    assert_eq!(receipt["schema_version"], 1);
    assert_eq!(receipt["charged_dispatches"], 0);
    assert_eq!(receipt["completion_reason"], "budget_exhausted");
    assert_eq!(receipt["completed"], false);
}

#[test]
fn comparison_resource_policy_conflicts_fail_before_dispatch() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[execution]\nmax_dispatches=0\n[execution.resources]\ncpus=2\n").unwrap();
    std::fs::write(project.path().join("good.toml"), "cpu_count=1").unwrap();
    std::fs::write(project.path().join("bad.toml"), "cpu_count=4").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .args([
            "compare",
            "--good",
            "good.toml",
            "--bad",
            "bad.toml",
            "--format",
            "json",
        ])
        .arg(project.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("conflict"),
        "{output:?}"
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn exhausted_doctor_is_operational_and_retains_partial_checks() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[execution]\nmax_dispatches=0\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .args(["doctor", "--format", "json"])
        .arg(project.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ready"], false);
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["name"] == "execution completion"
                && check["detail"] == "budget_exhausted")
    );
    assert_eq!(
        std::fs::read_dir(project.path().join(".reprobisect/executions"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn exhausted_comparison_returns_partial_evidence_instead_of_equivalence() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[execution]\nmax_dispatches=0\n").unwrap();
    std::fs::write(project.path().join("good.toml"), "timezone='UTC'").unwrap();
    std::fs::write(
        project.path().join("bad.toml"),
        "timezone='Pacific/Honolulu'",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .args([
            "compare",
            "--good",
            "good.toml",
            "--bad",
            "bad.toml",
            "--format",
            "json",
        ])
        .arg(project.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("exhausted comparison must retain partial evidence");
    assert_eq!(report["status"], "inconclusive");
    assert_eq!(report["good_runs"].as_array().unwrap().len(), 0);
    assert!(
        report["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n == "execution_completion=budget_exhausted")
    );
    assert_eq!(
        std::fs::read_dir(project.path().join(".reprobisect/executions"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn exhausted_fix_keeps_one_operation_receipt_and_exit_five() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[execution]\nmax_dispatches=0\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .args(["fix", "--verify", "--format", "json"])
        .arg(project.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnosis"]["status"], "inconclusive");
    assert!(
        report["verifications"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["verified"] == false)
    );
    assert_eq!(
        std::fs::read_dir(project.path().join(".reprobisect/executions"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn check_help_smoke_test() {
    let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .args(["check", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn ci_json_stdout_is_one_document() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .arg("check")
        .arg(temp.path().join("missing"))
        .arg("--ci")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("CI failure must still produce exactly one JSON document");
    assert_eq!(envelope["schema_version"], 1);
    assert_eq!(envelope["operation"], "check");
    assert_eq!(envelope["completion"]["reason"], "operational_error");
    assert!(envelope["diagnostic_status"].is_null());
    assert_eq!(envelope["policy"]["passed"], false);
}

#[test]
fn ci_usage_and_operational_errors_preserve_the_contract() {
    for (args, code, operation, reason, policy) in [
        (
            vec!["check", "--ci", "--not-a-flag"],
            2,
            "check",
            "usage_error",
            "require_reproducible",
        ),
        (
            vec![
                "diagnose",
                "--ci",
                "--ci-policy",
                "report-only",
                "definitely-missing-project",
            ],
            5,
            "diagnose",
            "operational_error",
            "report_only",
        ),
        (
            vec!["check", "--ci", "--ci-policy", "invalid"],
            2,
            "check",
            "usage_error",
            "require_reproducible",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(code), "{:?}", output);
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["operation"], operation);
        assert_eq!(value["completion"]["reason"], reason);
        assert_eq!(value["policy"]["name"], policy);
        assert_eq!(value["policy"]["exit_code"], code);
        assert_eq!(value["policy"]["passed"], false);
        assert_eq!(value["report"], serde_json::Value::Null);
        assert_eq!(
            value["identities"]["binary_version"],
            env!("CARGO_PKG_VERSION")
        );
        assert!(!output.stderr.contains(&0x1b));
        assert!(!output.stdout.contains(&0x1b));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .args(["check", "--ci-policy", "report-only"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn legacy_errors_and_historical_evidence_stay_separate() {
    let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .args(["check", "definitely-missing-project", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .args([
            "evidence",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/evidence/phase6-check-v5.json"
            ),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "reproducible");
    assert!(value.get("diagnostic_status").is_none());
    assert!(value.get("policy").is_none());
}

#[cfg(target_os = "linux")]
fn bounded_wait(mut child: std::process::Child) -> std::process::Output {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            panic!("execution failed to terminate within fixture deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    child.wait_with_output().unwrap()
}

#[cfg(target_os = "linux")]
fn owned_cleanup_contract(runtime: &str) {
    use sha2::{Digest, Sha256};
    let image = "docker.io/library/debian:bookworm";
    assert!(
        Command::new(runtime)
            .args(["pull", image])
            .status()
            .unwrap()
            .success()
    );
    let unrelated = format!("p02-unrelated-{}", uuid::Uuid::new_v4().simple());
    struct Unrelated<'a>(&'a str, String);
    impl Drop for Unrelated<'_> {
        fn drop(&mut self) {
            let _ = Command::new(self.0).args(["rm", "-f", &self.1]).status();
        }
    }
    let unrelated_guard = Unrelated(runtime, unrelated.clone());
    assert!(
        Command::new(runtime)
            .args([
                "run",
                "-d",
                "--name",
                &unrelated,
                "--label",
                "reprobisect.operation=unrelated-fixture",
                image,
                "sleep",
                "180"
            ])
            .status()
            .unwrap()
            .success()
    );
    for phase in ["attempt_timeout", "deadline_exhausted", "cancelled"] {
        let cancel = phase == "cancelled";
        let project = tempfile::tempdir().unwrap();
        let marker = format!("REPROBISECT_FIXTURE={}", uuid::Uuid::new_v4());
        let config = format!(
            r#"
[build]
runner = "{runtime}"
image = "{image}"
command = ["sh", "-c", "printf interrupted-artifact > out; printf started; sleep 60"]
outputs = ["out"]
timeout_seconds = {}
[build.env]
REPROBISECT_FIXTURE = "{}"
[execution]
total_timeout_seconds = {}
"#,
            if phase == "attempt_timeout" { 1 } else { 60 },
            marker.strip_prefix("REPROBISECT_FIXTURE=").unwrap(),
            if phase == "deadline_exhausted" {
                10
            } else {
                30
            }
        );
        std::fs::write(project.path().join(".reprobisect.toml"), &config).unwrap();
        std::fs::write(project.path().join("sentinel"), b"checkout unchanged").unwrap();
        let before = hex::encode(Sha256::digest(config.as_bytes()));
        let child = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
            .args(["check", "--ci", "--ci-policy", "report-only"])
            .arg(project.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut cancelled_identity = None;
        if cancel {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
            loop {
                let output = Command::new(runtime)
                    .args([
                        "ps",
                        "--no-trunc",
                        "--filter",
                        "label=reprobisect.operation",
                        "--format",
                        "{{.ID}}",
                    ])
                    .output()
                    .unwrap();
                assert!(output.status.success());
                for id in String::from_utf8_lossy(&output.stdout).lines() {
                    let inspected = Command::new(runtime)
                        .args(["container", "inspect", "--format", "{{json .}}", "--", id])
                        .output()
                        .unwrap();
                    if !inspected.status.success() {
                        continue;
                    }
                    let identity: serde_json::Value =
                        serde_json::from_slice(&inspected.stdout).unwrap();
                    let fixture_matches = identity["Config"]["Env"]
                        .as_array()
                        .is_some_and(|env| env.iter().any(|v| v == &marker));
                    let build_matches = identity["Name"].as_str().is_some_and(|name| {
                        name.trim_start_matches('/')
                            .starts_with("reprobisect-build-")
                    });
                    let artifact_started = identity["Mounts"].as_array().is_some_and(|mounts| {
                        mounts.iter().any(|mount| {
                            mount["Source"].as_str().is_some_and(|source| {
                                std::fs::read(std::path::Path::new(source).join("out"))
                                    .ok()
                                    .as_deref()
                                    == Some(b"interrupted-artifact")
                            })
                        })
                    });
                    if fixture_matches && build_matches && artifact_started {
                        let operation = identity["Config"]["Labels"]["reprobisect.operation"]
                            .as_str()
                            .unwrap();
                        uuid::Uuid::parse_str(operation).unwrap();
                        assert_eq!(identity["Id"], id);
                        cancelled_identity = Some((id.to_string(), operation.to_string()));
                        break;
                    }
                }
                if cancelled_identity.is_some() {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "owned build did not start"
                );
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            assert!(
                Command::new("kill")
                    .args(["-INT", &child.id().to_string()])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let output = bounded_wait(child);
        let phase_name = format!("{runtime}/{phase}");
        let details = execution_failure_details(&phase_name, &serde_json::Value::Null, &output);
        assert_eq!(output.status.code(), Some(5), "{details}");
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["completion"]["reason"], "operational_error");
        assert_eq!(value["completion"]["completed"], false);
        assert_eq!(value["coverage"]["complete"], false);
        assert_eq!(value["policy"]["passed"], false);
        let partial: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                project
                    .path()
                    .join(value["report"]["path"].as_str().unwrap()),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(partial["status"], "inconclusive");
        assert!(
            partial["notes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n == &format!("execution_completion={}", phase))
        );
        let receipts = std::fs::read_dir(project.path().join(".reprobisect/executions"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(receipts.len(), 1);
        let receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(receipts[0].path()).unwrap()).unwrap();
        let details = execution_failure_details(&phase_name, &receipt, &output);
        if let Some((id, operation)) = cancelled_identity {
            assert_eq!(receipt["operation_id"], operation);
            assert!(
                receipt["owned_containers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v["purpose"] == "build" && v["container_id"] == id)
            );
        }
        let attempt_path = receipt["partial_attempts"][0].as_str().unwrap();
        let attempt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(project.path().join(attempt_path)).unwrap())
                .unwrap();
        assert!(
            attempt["stdout"]["text"]
                .as_str()
                .unwrap()
                .contains("started")
        );
        assert!(
            attempt["artifacts"]["observations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["logical_path"] == "out"
                    && v["retained_bytes_hex"] == hex::encode(b"interrupted-artifact"))
        );
        assert_eq!(receipt["completion_reason"], phase, "{details}");
        assert_eq!(receipt["completed"], false, "{details}");
        assert!(
            receipt["charged_dispatches"].as_u64().unwrap() > 0,
            "{details}"
        );
        assert!(
            receipt["owned_containers"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["cleanup_verified"] == true),
            "{details}"
        );
        let label = format!(
            "label=reprobisect.operation={}",
            receipt["operation_id"].as_str().unwrap()
        );
        let owned = Command::new(runtime)
            .args(["ps", "--filter", &label, "--format", "{{.Names}}"])
            .output()
            .unwrap();
        assert!(owned.status.success());
        assert!(
            owned.stdout.is_empty(),
            "owned containers still running: {owned:?}"
        );
        let other = Command::new(runtime)
            .args(["inspect", "--format", "{{.State.Running}}", &unrelated])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&other.stdout).trim(), "true");
        assert_eq!(
            hex::encode(Sha256::digest(
                std::fs::read(project.path().join(".reprobisect.toml")).unwrap()
            )),
            before
        );
        assert_eq!(
            std::fs::read(project.path().join("sentinel")).unwrap(),
            b"checkout unchanged"
        );
        assert!(!project.path().join("out").exists());
    }
    drop(unrelated_guard);
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires Docker; explicitly run in fixtures CI"]
fn p02_live_docker_cancel_kills_owned_container_and_keeps_partial_evidence() {
    owned_cleanup_contract("docker");
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires Podman; explicitly run in podman CI"]
fn p02_live_podman_cancel_kills_owned_container_and_keeps_partial_evidence() {
    owned_cleanup_contract("podman");
}

#[cfg(target_os = "linux")]
fn fixture(name: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    for entry in walkdir::WalkDir::new(&root) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let dest = temp.path().join(entry.path().strip_prefix(&root).unwrap());
            std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
    temp
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires Docker; run explicitly in the fixtures CI job"]
fn ci_live_success_and_finding_policy_matrix() {
    use sha2::{Digest, Sha256};
    for (name, status, diagnostic_code) in [
        ("reproducible-c", "reproducible", 0),
        ("source-path-c", "non_reproducible", 1),
    ] {
        for (operation, policy) in [
            ("check", "require-reproducible"),
            ("diagnose", "report-only"),
        ] {
            let project = fixture(name);
            let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
                .args([
                    operation,
                    "--ci",
                    "--ci-policy",
                    policy,
                    "--format",
                    "text",
                    "--verbose",
                ])
                .arg(project.path())
                .output()
                .unwrap();
            let expected = if policy == "report-only" {
                0
            } else {
                diagnostic_code
            };
            assert_eq!(output.status.code(), Some(expected), "{:?}", output);
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["schema_version"], 1);
            assert_eq!(value["operation"], operation);
            assert_eq!(value["diagnostic_status"], status);
            assert_eq!(value["coverage"]["complete"], true);
            assert_eq!(value["policy"]["exit_code"], expected);
            assert_eq!(value["policy"]["diagnostic_exit_code"], diagnostic_code);
            let bytes = std::fs::read(
                project
                    .path()
                    .join(value["report"]["path"].as_str().unwrap()),
            )
            .unwrap();
            assert_eq!(
                value["report"]["sha256"],
                hex::encode(Sha256::digest(&bytes))
            );
            let report: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(report["status"], status);
            assert!(report.get("policy").is_none());
            let binary = std::fs::read(env!("CARGO_BIN_EXE_reprobisect")).unwrap();
            assert_eq!(
                value["identities"]["binary_sha256"],
                hex::encode(Sha256::digest(binary))
            );
            assert!(!output.stdout.contains(&0x1b));
        }
        let project = fixture(name);
        let legacy = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
            .args(["check", "--format", "json"])
            .arg(project.path())
            .output()
            .unwrap();
        assert_eq!(legacy.status.code(), Some(diagnostic_code), "{:?}", legacy);
        let value: serde_json::Value = serde_json::from_slice(&legacy.stdout).unwrap();
        assert_eq!(value["status"], status);
        assert!(value.get("policy").is_none());
    }
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires Docker; run explicitly in the fixtures CI job"]
fn ci_live_unstable_controls_are_incomplete() {
    let project = fixture("source-path-c");
    let config = std::fs::read_to_string(project.path().join(".reprobisect.toml")).unwrap();
    let config = config.replace(
        "mkdir -p {build}/build && gcc -g -O0 -o {build}/build/app {source}/main.c",
        "mkdir -p {build}/build && cat /proc/sys/kernel/random/uuid > {build}/build/app",
    );
    std::fs::write(project.path().join(".reprobisect.toml"), config).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .args(["check", "--ci", "--ci-policy", "report-only"])
        .arg(project.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{:?}", output);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["diagnostic_status"], "uncontrolled_nondeterminism");
    assert_eq!(value["completion"]["reason"], "incomplete_coverage");
    assert_eq!(value["coverage"]["interventions"]["planned"], 1);
    assert_eq!(value["coverage"]["interventions"]["skipped"], 1);
    assert_eq!(value["policy"]["passed"], false);
}

#[test]
fn ci_config_error_is_not_non_reproducible() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "not valid TOML").unwrap();
    for policy in ["require-reproducible", "report-only"] {
        let output = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
            .args(["check", "--ci", "--ci-policy", policy])
            .arg(project.path())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(5));
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["diagnostic_status"], serde_json::Value::Null);
        assert_eq!(value["completion"]["reason"], "operational_error");
        assert_eq!(value["policy"]["passed"], false);
        assert_eq!(value["coverage"], serde_json::Value::Null);
        assert!(!project.path().join(".reprobisect").exists());
    }
}

#[test]
fn public_postbaseline_build_spawn_failure_stops_scheduling() {
    for route in ["check", "diagnose"] {
        for policy in [Some("report-only"), Some("require-reproducible"), None] {
            let project = tempfile::tempdir().unwrap();
            let runtime = tempfile::tempdir().unwrap();
            let fallback = tempfile::tempdir().unwrap();
            let executable = format!("docker{}", std::env::consts::EXE_SUFFIX);
            for dir in [runtime.path(), fallback.path()] {
                std::fs::copy(runtime_double().join(&executable), dir.join(&executable)).unwrap();
            }
            let path = std::env::join_paths(
                [runtime.path().to_path_buf(), fallback.path().to_path_buf()]
                    .into_iter()
                    .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
            )
            .unwrap();
            std::fs::write(
                project.path().join(".reprobisect.toml"),
                r#"
[build]
image='unused'
command=['true']
outputs=['out']
timeout_seconds=1
[experiments]
control_runs=2
intervention_runs=2
confirmation_runs=1
stochastic_runs=4
interaction_runs=2
[experiments.environment_variables]
POSTBASELINE_DIMENSION=['A','B']
[experiments.dimensions]
network_access=false
source_path=false
build_path=false
source_date_epoch=false
timezone=false
locale=false
hostname=true
source_mtime=false
cpu_count=false
umask=false
directory_order=false
[execution]
max_dispatches=4096
total_timeout_seconds=60
"#,
            )
            .unwrap();
            let mut command = double_command(project.path(), "failed-build-spawn");
            command.env("PATH", &path).arg(route);
            if let Some(policy) = policy {
                command.args(["--ci", "--ci-policy", policy]);
            } else {
                command.args(["--format", "json"]);
            }
            let output = command.arg(project.path()).output().unwrap();
            let receipt = execution_receipt(project.path());
            let details = execution_failure_details("postbaseline-build-spawn", &receipt, &output);
            assert!(
                project.path().join("postbaseline-spawn-armed").is_file(),
                "{details}"
            );
            assert_eq!(
                std::fs::read_to_string(project.path().join("build-count")).unwrap(),
                "2",
                "{details}"
            );
            let log = std::fs::read_to_string(project.path().join("runtime.log")).unwrap();
            assert_eq!(
                log.lines()
                    .filter(|line| line.starts_with("run |") && line.contains("reprobisect-build-"))
                    .count(),
                2,
                "{log}; {details}"
            );
            let error = Command::new("docker")
                .env("PATH", &path)
                .env("PROBE_ROOT", project.path())
                .env("PROBE_MODE", "failed-build-spawn")
                .arg("version")
                .spawn()
                .expect_err(
                    "must fail at OS spawn before any child, not an executed build failure",
                );
            #[cfg(target_os = "linux")]
            assert_eq!(error.raw_os_error(), Some(40), "fatal ELOOP: {error}");
            #[cfg(not(target_os = "linux"))]
            let _ = error;
            assert_eq!(
                std::fs::read_to_string(project.path().join("runtime.log")).unwrap(),
                log
            );
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            let report = if policy.is_some() {
                serde_json::from_slice::<serde_json::Value>(
                    &std::fs::read(
                        project
                            .path()
                            .join(value["report"]["path"].as_str().unwrap()),
                    )
                    .unwrap(),
                )
                .unwrap()
            } else {
                value.clone()
            };
            assert_eq!(report["runs"].as_array().unwrap().len(), 2, "{details}");
            assert_eq!(
                output.status.code(),
                Some(5),
                "{route}/{policy:?}: {details}"
            );
            assert_eq!(receipt["completed"], false, "{details}");
            assert_eq!(
                receipt["completion_reason"], "operational_error",
                "{details}"
            );
            assert_eq!(
                receipt["charged_dispatches"], 48,
                "complete first promotion group charged without refund; {details}"
            );
            assert_eq!(
                report["interventions"].as_array().unwrap().len(),
                1,
                "no second dimension after failed spawn; {details}"
            );
            let owned = receipt["owned_containers"].as_array().unwrap();
            assert_eq!(
                owned.len(),
                4,
                "probe, two baselines and one non-dispatched build; {details}"
            );
            assert!(
                owned[..3]
                    .iter()
                    .all(|v| v["creation_outcome"] == "acknowledged"
                        && v["cleanup_verified"] == true),
                "{details}"
            );
            assert_eq!(owned[3]["creation_outcome"], "not_dispatched", "{details}");
            assert!(owned[3]["container_id"].is_null(), "{details}");
            assert_eq!(owned[3]["cleanup_verified"], true, "{details}");
            assert_eq!(
                log.lines().filter(|line| line.starts_with("rm |")).count(),
                3,
                "no removal against the failed spawn; {log}"
            );
            if policy.is_some() {
                assert_eq!(value["completion"]["completed"], false, "{details}");
                assert_eq!(
                    value["completion"]["reason"], "operational_error",
                    "{details}"
                );
                assert_eq!(value["coverage"]["complete"], false, "{details}");
                assert_eq!(value["policy"]["passed"], false, "{details}");
            }
        }
    }
}

#[test]
fn public_fix_interrupted_patch_verification_keeps_partial_evidence() {
    for mode in ["fix-verify-stable", "fix-verify-timeout"] {
        let project = tempfile::tempdir().unwrap();
        let runtime = tempfile::tempdir().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join(".reprobisect.toml"),
            r#"
[build]
image='unused'
command=['make']
outputs=['out']
timeout_seconds=1
log_capture_max_bytes=65536
[experiments]
control_runs=2
intervention_runs=2
confirmation_runs=1
stochastic_runs=4
interaction_runs=2
[experiments.dimensions]
network_access=false
source_path=false
build_path=false
source_date_epoch=true
timezone=false
locale=false
hostname=false
source_mtime=false
cpu_count=false
umask=false
directory_order=false
[execution]
max_dispatches=4096
total_timeout_seconds=60
"#,
        )
        .unwrap();
        std::fs::write(
            project.path().join("Makefile"),
            "all:\n\tprintf artifact > out\n",
        )
        .unwrap();
        std::fs::write(
            project.path().join("source.txt"),
            "unchanged original project\n",
        )
        .unwrap();
        let originals: Vec<_> = [".reprobisect.toml", "Makefile", "source.txt"]
            .into_iter()
            .map(|name| (name, std::fs::read(project.path().join(name)).unwrap()))
            .collect();
        let output = double_command(project.path(), mode)
            .env("PROBE_ROOT", runtime.path())
            .env("TMPDIR", temporary.path())
            .env("TMP", temporary.path())
            .env("TEMP", temporary.path())
            .args(["fix", "--verify", "--format", "json"])
            .arg(project.path())
            .output()
            .unwrap();
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let receipt = execution_receipt(project.path());
        let details = execution_failure_details("patched-copy-verification", &receipt, &output);
        for (name, bytes) in originals {
            assert_eq!(
                std::fs::read(project.path().join(name)).unwrap(),
                bytes,
                "original checkout changed: {name}; {details}"
            );
        }
        assert_eq!(
            std::fs::read_dir(project.path().join(".reprobisect/executions"))
                .unwrap()
                .count(),
            1,
            "one shared receipt; {details}"
        );
        assert_eq!(
            report["diagnosis"]["runs"].as_array().unwrap().len(),
            2,
            "{details}"
        );
        assert_eq!(
            report["diagnosis"]["interventions"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "{details}"
        );
        assert_eq!(
            report["diagnosis"]["interventions"][0]["runs"]
                .as_array()
                .unwrap()
                .len(),
            2,
            "{details}"
        );
        assert_eq!(
            report["diagnosis"]["interventions"][0]["reverted_to_baseline"], true,
            "{details}"
        );
        assert_eq!(
            report["candidates"].as_array().unwrap().len(),
            1,
            "{details}"
        );
        assert_eq!(
            report["candidates"][0]["project_patch"]["path"], "Makefile",
            "{details}"
        );
        assert_eq!(
            report["verifications"].as_array().unwrap().len(),
            1,
            "{details}"
        );
        assert_eq!(report["verifications"][0]["attempted"], true, "{details}");
        assert!(
            std::fs::read_to_string(runtime.path().join("patched-makefile-observed"))
                .unwrap()
                .contains("# ReproBisect candidate:"),
            "verification must use patched copy; {details}"
        );
        assert!(
            std::fs::read_dir(temporary.path())
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("reprobisect-fix-verify-")),
            "patch TempDir must already have unwound; {details}"
        );
        assert!(
            receipt["owned_containers"]
                .as_array()
                .unwrap()
                .iter()
                .all(|owned| owned["creation_outcome"] == "acknowledged"
                    && owned["cleanup_verified"] == true),
            "exact-ID cleanup; {details}"
        );
        if mode == "fix-verify-stable" {
            assert_eq!(
                std::fs::read_to_string(runtime.path().join("build-count")).unwrap(),
                "9",
                "five diagnosis plus four verification builds; {details}"
            );
            assert_eq!(output.status.code(), Some(0), "{details}");
            assert_eq!(receipt["completed"], true, "{details}");
            assert_eq!(report["verifications"][0]["verified"], true, "{details}");
            assert!(
                receipt["partial_attempts"].as_array().unwrap().is_empty(),
                "{details}"
            );
        } else {
            assert_eq!(
                std::fs::read_to_string(runtime.path().join("build-count")).unwrap(),
                "6",
                "first patched-copy build reached before interruption; {details}"
            );
            assert_eq!(output.status.code(), Some(5), "{details}");
            assert_eq!(receipt["completed"], false, "{details}");
            assert_eq!(receipt["completion_reason"], "attempt_timeout", "{details}");
            assert_eq!(report["verifications"][0]["verified"], false, "{details}");
            assert!(
                report["verifications"][0]["error"]
                    .as_str()
                    .unwrap()
                    .contains("attempt_timeout"),
                "{details}"
            );
            let references = receipt["partial_attempts"].as_array().unwrap();
            assert_eq!(references.len(), 1, "{details}");
            for relative in references {
                let path = project.path().join(relative.as_str().unwrap());
                assert!(
                    path.is_file(),
                    "partial evidence must survive at original root after temporary-copy deletion: {}; {details}",
                    path.display()
                );
                let partial: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                assert_eq!(
                    partial["operation_id"], receipt["operation_id"],
                    "{details}"
                );
                assert_eq!(partial["purpose"], "build", "{details}");
                assert_eq!(partial["completed"], false, "{details}");
                assert_eq!(partial["completion_reason"], "attempt_timeout", "{details}");
                assert_eq!(
                    partial["ordinal"], 1,
                    "first build in new verification experiment; {details}"
                );
                assert_eq!(partial["cleanup_verified"], true, "{details}");
                assert!(
                    partial["stdout"]["observed_bytes"].as_u64().unwrap() > 0,
                    "{details}"
                );
                assert!(
                    partial["stdout"]["text"].as_str().unwrap().len() <= 65536,
                    "{details}"
                );
                assert_eq!(
                    partial["artifacts"]["artifact_completion"], "unknown",
                    "{details}"
                );
                assert!(
                    partial["artifacts"]["observations"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|artifact| artifact["logical_path"] == "out"
                            && artifact["stable_snapshot"] == false
                            && artifact["artifact_completion"] == "unknown"),
                    "bounded artifact observations must also survive; {details}"
                );
            }
        }
    }
}

// P03 fixtures are private copies; canonical historical fixtures stay untouched.
fn p03_input(root: &std::path::Path) -> std::path::PathBuf {
    let mut value: serde_json::Value =
        serde_json::from_slice(include_bytes!("evidence/phase6-check-v5.json")).unwrap();
    let secret = "P03_SECRET_do_not_disclose";
    value["source_digest"] = secret.into();
    value["notes"] = serde_json::json!([secret]);
    value["baseline_environment"]["hostname"] = secret.into();
    value["baseline_environment"]["environment"] = serde_json::json!({"TOKEN": secret});
    value["runs"][0]["stdout"] = secret.into();
    value["runs"][0]["stderr"] = secret.into();
    value["runs"][0]["command"] = serde_json::json!([secret]);
    value["runs"][0]["image"] = secret.into();
    value["runs"][0]["working_directory"] = secret.into();
    value["runs"][0]["effective_environment"] = serde_json::json!({"TOKEN": secret});
    value["runs"][0]["controlled_environment"]["hostname"] = secret.into();
    value["runs"][0]["artifacts"][0]["logical_path"] = secret.into();
    value["runs"][0]["artifacts"][0]["sha256"] = "a".repeat(64).into();
    value["runs"][0]["artifacts"][0]["semantic_metadata"] = serde_json::json!({
        "kind": "python_wheel", "attributes": {"Name": secret, "Version": secret, "private": secret}
    });
    value["interventions"] = serde_json::json!([{
        "intervention": {"id": secret, "kind": "hostname", "variable": secret,
            "baseline_value": secret, "variant_value": secret, "description": secret},
        "runs": [], "artifact_deltas": [], "changed": false,
        "reverted_to_baseline": null, "confirmation_run": null, "error": null
    }]);
    let path = root.join("input.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    path
}

fn p03_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_reprobisect"))
}

#[test]
fn summary_omits_raw_logs_and_controlled_secrets() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let input = p03_input(root.path());
    let original = std::fs::read(&input).unwrap();
    let empty_path = root.path().join("no-runtime");
    std::fs::create_dir(&empty_path).unwrap();
    let output = p03_command()
        .env("PATH", empty_path)
        .args(["summary", "--format", "json"])
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "summary must be offline: {output:?}"
    );
    let view: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(view["kind"], "check_report");
    assert_eq!(view["diagnostic_status"], "reproducible");
    assert_eq!(view["coverage"]["status"], "unknown");
    assert_eq!(view["completion"], "unknown");
    assert_eq!(
        view["original_input_sha256"],
        hex::encode(Sha256::digest(&original))
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        !text.contains("P03_SECRET"),
        "default view leaked private data: {text}"
    );
    assert!(!text.contains(&root.path().to_string_lossy().to_string()));
    assert!(
        view["excluded"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s == "raw_logs")
    );
    assert_eq!(std::fs::read(&input).unwrap(), original);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
}

#[test]
fn summary_escapes_workflow_commands() {
    let root = tempfile::tempdir().unwrap();
    let input = p03_input(root.path());
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&input).unwrap()).unwrap();
    let attack = "\n::error::injected\r\n##vso[task.setvariable variable=secret]yes\n<script>alert(1)</script>\u{1b}[31m [link](https://evil) `code` ![x](url) &";
    value["interventions"][0]["intervention"]["baseline_value"] = attack.into();
    value["interventions"][0]["intervention"]["variant_value"] = attack.into();
    std::fs::write(&input, serde_json::to_vec(&value).unwrap()).unwrap();
    for format in ["json", "markdown"] {
        let output = p03_command()
            .args([
                "summary",
                "--format",
                format,
                "--allow-field",
                "controlled-values",
            ])
            .arg(&input)
            .output()
            .unwrap();
        assert!(output.status.success(), "safe renderer missing: {output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains('\u{1b}'));
        assert!(!text.contains("<script>"));
        assert!(
            !text
                .lines()
                .any(|l| l.starts_with("::") || l.starts_with("##vso["))
        );
        if format == "json" {
            let view: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(
                view["controlled_values"][0]["baseline"], attack,
                "escaped encoding must retain exact selected value"
            );
        } else {
            assert!(!text.contains("[link](https://evil)"));
            assert!(text.contains("&#58;&#58;error&#58;&#58;"));
            assert!(text.contains("&lt;script&gt;"));
        }
    }
}

fn p03_preview(
    input: &std::path::Path,
    destination: &std::path::Path,
    fields: &[&str],
) -> serde_json::Value {
    let mut command = p03_command();
    command
        .arg("export")
        .arg(input)
        .arg("--output")
        .arg(destination);
    for field in fields {
        command.args(["--allow-field", field]);
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "preview failed: {output:?}");
    assert!(
        output.stdout.len() <= 1024 * 1024,
        "escaped preview must be bounded too"
    );
    let text = std::str::from_utf8(&output.stdout).unwrap();
    assert!(!text.chars().any(|c| matches!(c, '\u{7f}'..='\u{9f}' | '\u{2028}' | '\u{2029}' | '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn export_preserves_original_evidence_digest() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let input = p03_input(root.path());
    let original = std::fs::read(&input).unwrap();
    let destination = root.path().join("bundle");
    let preview = p03_preview(&input, &destination, &[]);
    assert!(
        !destination.exists(),
        "preview must not create the destination"
    );
    assert_eq!(
        preview["original_input_sha256"],
        hex::encode(Sha256::digest(&original))
    );
    assert_eq!(preview["allowed_fields"], serde_json::json!([]));
    let members = preview["members"].as_array().unwrap();
    assert_eq!(
        members
            .iter()
            .map(|m| m["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["summary.json", "summary.md", "manifest.json"]
    );
    for member in members {
        let bytes = member["bytes"].as_str().unwrap().as_bytes();
        assert_eq!(member["sha256"], hex::encode(Sha256::digest(bytes)));
        assert_eq!(member["size_bytes"].as_u64().unwrap(), bytes.len() as u64);
        assert!(!String::from_utf8_lossy(bytes).contains("P03_SECRET"));
        assert!(
            !String::from_utf8_lossy(bytes).contains(&root.path().to_string_lossy().to_string())
        );
    }
    let output = p03_command()
        .arg("export")
        .arg(&input)
        .arg("--output")
        .arg(&destination)
        .arg("--approve")
        .arg(preview["approval_sha256"].as_str().unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "approved export failed: {output:?}"
    );
    assert_eq!(std::fs::read(&input).unwrap(), original);
    assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 3);
    for member in members {
        assert_eq!(
            std::fs::read(destination.join(member["name"].as_str().unwrap())).unwrap(),
            member["bytes"].as_str().unwrap().as_bytes()
        );
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(destination.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(
        manifest["original_input_sha256"],
        preview["original_input_sha256"]
    );
    assert_eq!(manifest["policy_version"], "p03-disclosure-v1");
    assert_eq!(manifest["renderer_version"], "p03-renderer-v1");
    assert_eq!(manifest["members"].as_array().unwrap().len(), 2);
    assert_ne!(
        preview["original_input_sha256"],
        preview["members"][0]["sha256"]
    );
}

fn p03_directory_link(target: &std::path::Path, link: &std::path::Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        // Junctions need no developer-mode symlink privilege; exercise real reparse ancestors.
        let output = Command::new("C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "New-Item",
                "-ItemType",
                "Junction",
                "-Path",
            ])
            .arg(link)
            .arg("-Target")
            .arg(target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "junction fixture failed: {output:?}"
        );
    }
}

#[test]
fn export_refuses_symlink_and_parent_escape() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let input = p03_input(&real);
    let alias = root.path().join("alias");
    p03_directory_link(&real, &alias);
    let child = real.join("child");
    std::fs::create_dir(&child).unwrap();
    for (index, source, destination) in [
        (0, alias.join("input.json"), real.join("bundle-0")),
        (1, input.clone(), alias.join("bundle-1")),
        (2, input.clone(), child.join("..").join("bundle-2")),
        (
            3,
            child.join("..").join("input.json"),
            real.join("bundle-3"),
        ),
    ] {
        let actual = real.join(format!("bundle-{index}"));
        let approval = p03_preview(&input, &actual, &[]);
        let output = p03_command()
            .arg("export")
            .arg(&source)
            .arg("--output")
            .arg(&destination)
            .arg("--approve")
            .arg(approval["approval_sha256"].as_str().unwrap())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(5),
            "path defense {index}: {output:?}"
        );
        assert!(!actual.exists(), "path rejection must precede all writes");
        assert!(output.stdout.is_empty());
    }
    #[cfg(unix)]
    {
        let link = root.path().join("input-link.json");
        std::os::unix::fs::symlink(&input, &link).unwrap();
        let destination = real.join("bundle-file-link");
        let approval = p03_preview(&input, &destination, &[]);
        let output = p03_command()
            .arg("export")
            .arg(&link)
            .arg("--output")
            .arg(&destination)
            .arg("--approve")
            .arg(approval["approval_sha256"].as_str().unwrap())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(5));
        assert!(!destination.exists());
    }
}

#[test]
fn summary_supports_historical_evidence_kinds() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    for (file, kind) in [
        ("phase6-check-v5.json", "check_report"),
        ("phase6-build-failure-v1.json", "build_failure"),
        ("phase8-build-run-v3.json", "build_run"),
        ("phase18-build-run-v10.json", "build_run"),
        ("phase7-fix-v3.json", "fix_report"),
        (
            "phase14-comparison-v1.json",
            "environment_comparison_report",
        ),
    ] {
        let original = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/evidence")
                .join(file),
        )
        .unwrap();
        let input = root.path().join(file);
        std::fs::write(&input, &original).unwrap();
        let output = p03_command().arg("summary").arg(&input).output().unwrap();
        assert!(output.status.success(), "supported kind {kind}: {output:?}");
        let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(summary["kind"], kind);
        assert_eq!(
            summary["original_input_sha256"],
            hex::encode(Sha256::digest(&original))
        );
        assert_eq!(summary["coverage"]["status"], "unknown");
        assert!(summary.get("stdout").is_none());
        assert!(summary.get("diagnosis").is_none());
        if kind == "build_failure" {
            assert!(summary["build_exit_code"].as_i64().unwrap() != 0);
        }
        if kind == "environment_comparison_report" {
            assert_eq!(summary["comparison_status"], "equivalent");
        }
        let destination = root.path().join(format!("{file}-bundle"));
        let preview = p03_preview(&input, &destination, &[]);
        let output = p03_command()
            .arg("export")
            .arg(&input)
            .arg("--output")
            .arg(&destination)
            .arg("--approve")
            .arg(preview["approval_sha256"].as_str().unwrap())
            .output()
            .unwrap();
        assert!(output.status.success(), "export kind {kind}: {output:?}");
        assert_eq!(std::fs::read(&input).unwrap(), original);
    }
}

#[test]
fn summary_selection_is_explicit() {
    let root = tempfile::tempdir().unwrap();
    let input = p03_input(root.path());
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&input).unwrap()).unwrap();
    value["runs"][0]["artifacts"][0]["semantic_metadata"]["attributes"]["metadata_version"] =
        "P03_META_ALLOWED".into();
    value["runs"][0]["effective_environment"] =
        serde_json::json!({"PRIVATE_ONLY_ENV": "NEVER_SHARE_ENV"});
    std::fs::write(&input, serde_json::to_vec(&value).unwrap()).unwrap();
    let selected = p03_command()
        .arg("summary")
        .arg(&input)
        .args(["--allow-field", "controlled-values"])
        .output()
        .unwrap();
    assert!(selected.status.success());
    let selected: serde_json::Value = serde_json::from_slice(&selected.stdout).unwrap();
    assert!(
        !selected["excluded"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "controlled_values"),
        "selected values must not be labelled excluded"
    );
    assert_eq!(
        selected["controlled_values"][0]["baseline"],
        "P03_SECRET_do_not_disclose"
    );
    let destination = root.path().join("selected");
    let preview = p03_preview(
        &input,
        &destination,
        &[
            "package-metadata",
            "artifact-paths",
            "controlled-values",
            "artifact-paths",
        ],
    );
    assert_eq!(
        preview["allowed_fields"],
        serde_json::json!(["controlled-values", "artifact-paths", "package-metadata"])
    );
    let view: serde_json::Value =
        serde_json::from_str(preview["members"][0]["bytes"].as_str().unwrap()).unwrap();
    assert_eq!(
        view["artifacts"][0]["logical_path"],
        "P03_SECRET_do_not_disclose"
    );
    assert_eq!(
        view["artifacts"][0]["package_metadata"]["metadata_version"],
        "P03_META_ALLOWED"
    );
    assert!(
        view["artifacts"][0]["package_metadata"]
            .get("Name")
            .is_none()
    );
    assert!(
        !serde_json::to_string(&preview)
            .unwrap()
            .contains("NEVER_SHARE_ENV")
    );
    assert_eq!(
        preview["allowed_fields"],
        p03_preview(
            &input,
            &destination,
            &["controlled-values", "artifact-paths", "package-metadata"]
        )["allowed_fields"]
    );
    for selector in [
        "raw-logs",
        "source",
        "binaries",
        "ordinary-environment",
        "/runs/0/stdout",
    ] {
        let output = p03_command()
            .arg("export")
            .arg(&input)
            .arg("--output")
            .arg(&destination)
            .args(["--allow-field", selector])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!destination.exists());
    }
}

#[test]
fn summary_retains_precise_incomplete_execution() {
    let root = tempfile::tempdir().unwrap();
    for file in [
        "phase6-check-v5.json",
        "phase7-fix-v3.json",
        "phase14-comparison-v1.json",
    ] {
        let original: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/evidence")
                    .join(file),
            )
            .unwrap(),
        )
        .unwrap();
        for reason in [
            "budget_exhausted",
            "deadline_exhausted",
            "cancelled",
            "attempt_timeout",
            "cleanup_failed",
            "creation_unknown",
            "operational_error",
        ] {
            let mut value = original.clone();
            value["notes"] = serde_json::json!([
                format!("execution_completion={reason}"),
                "PRIVATE_FREE_FORM"
            ]);
            let input = root.path().join("partial.json");
            std::fs::write(&input, serde_json::to_vec(&value).unwrap()).unwrap();
            let output = p03_command().arg("summary").arg(&input).output().unwrap();
            assert!(output.status.success(), "{output:?}");
            let view: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(view["completion"], "incomplete", "{file}/{reason}");
            assert_eq!(view["execution_completed"], false);
            assert_eq!(view["execution_reason"], reason);
            assert_eq!(view["coverage"]["complete"], false);
            assert!(view["coverage"]["authoritative"].is_null());
            assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE_FREE_FORM"));
        }
    }
    let input = p03_input(root.path());
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&input).unwrap()).unwrap();
    value["runs"] = serde_json::json!([]);
    value["notes"] = serde_json::json!(["execution_completion=unrecognized_private_text"]);
    std::fs::write(&input, serde_json::to_vec(&value).unwrap()).unwrap();
    let output = p03_command().arg("summary").arg(&input).output().unwrap();
    let view: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        view["diagnostic_status"], "reproducible",
        "do not rewrite persisted diagnostic truth"
    );
    assert_eq!(view["completion"], "unknown");
    assert!(view["execution_completed"].is_null());
    assert_eq!(
        view["coverage"]["status"], "unknown",
        "an isolated report cannot reconstruct the plan"
    );
    assert!(view["coverage"]["complete"].is_null());
}

#[test]
fn ci_sidecar_retains_authoritative_incomplete_coverage() {
    use sha2::{Digest, Sha256};
    for operation in ["check", "diagnose"] {
        for limit in ["max_dispatches = 0", "total_timeout_seconds = 0"] {
            let project = tempfile::tempdir().unwrap();
            std::fs::write(project.path().join(".reprobisect.toml"), format!("[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[execution]\n{limit}\n")).unwrap();
            let sidecar = project.path().join("summary.json");
            let output = p03_command()
                .arg(operation)
                .arg(project.path())
                .args(["--ci", "--ci-policy", "report-only", "--summary-output"])
                .arg(&sidecar)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(5), "partial CI: {output:?}");
            let envelope: serde_json::Value = serde_json::from_slice(&output.stdout)
                .expect("one frozen envelope, not a second JSON summary");
            assert_eq!(envelope["completion"]["completed"], false);
            let view: serde_json::Value = serde_json::from_slice(
                &std::fs::read(&sidecar).expect("opt-in partial sidecar must exist"),
            )
            .unwrap();
            assert_eq!(view["coverage"]["authoritative"], envelope["coverage"]);
            assert_eq!(view["coverage"]["complete"], false);
            assert_eq!(view["execution_completed"], false);
            assert_eq!(
                view["execution_reason"],
                if limit.starts_with("max") {
                    "budget_exhausted"
                } else {
                    "deadline_exhausted"
                }
            );
            assert_eq!(view["diagnostic_status"], envelope["diagnostic_status"]);
            let report = std::fs::read(
                project
                    .path()
                    .join(envelope["report"]["path"].as_str().unwrap()),
            )
            .unwrap();
            assert_eq!(
                view["original_input_sha256"],
                hex::encode(Sha256::digest(&report))
            );
            assert_eq!(view["original_input_sha256"], envelope["report"]["sha256"]);
            assert!(
                !String::from_utf8_lossy(&std::fs::read(&sidecar).unwrap())
                    .contains(&project.path().to_string_lossy().to_string())
            );
            let sentinel = b"create-only sidecar";
            std::fs::write(&sidecar, sentinel).unwrap();
            let failed = p03_command()
                .arg(operation)
                .arg(project.path())
                .args(["--ci", "--summary-output"])
                .arg(&sidecar)
                .output()
                .unwrap();
            assert_eq!(failed.status.code(), Some(5));
            let failed: serde_json::Value = serde_json::from_slice(&failed.stdout).unwrap();
            assert_eq!(failed["completion"]["reason"], "operational_error");
            assert!(
                failed["diagnostic_status"].is_null(),
                "generic sidecar operational error must not invent a result"
            );
            assert_eq!(std::fs::read(&sidecar).unwrap(), sentinel);
        }
    }
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[execution]\nmax_dispatches=0\n").unwrap();
    let sidecar = project.path().join("ordinary-summary.json");
    let output = p03_command()
        .arg("check")
        .arg(project.path())
        .args(["--format", "json", "--summary-output"])
        .arg(&sidecar)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "inconclusive");
    assert!(report.get("policy").is_none());
    assert!(sidecar.exists());
}

#[test]
fn selected_rendering_escapes_unicode_line_and_bidi_controls() {
    let root = tempfile::tempdir().unwrap();
    let input = p03_input(root.path());
    let attack = "safe\u{85}::error::injected\u{2028}##vso[x]\u{2029}<script>\u{202e}\u{2066}\u{2069}\u{061c}\u{200e}\u{200f}";
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&input).unwrap()).unwrap();
    value["interventions"][0]["intervention"]["baseline_value"] = attack.into();
    value["runs"][0]["artifacts"][0]["logical_path"] = attack.into();
    value["runs"][0]["artifacts"][0]["semantic_metadata"]["attributes"]["metadata_version"] =
        attack.into();
    std::fs::write(&input, serde_json::to_vec(&value).unwrap()).unwrap();
    for format in ["json", "markdown"] {
        let output = p03_command()
            .arg("summary")
            .arg(&input)
            .args([
                "--format",
                format,
                "--allow-field",
                "controlled-values",
                "--allow-field",
                "artifact-paths",
                "--allow-field",
                "package-metadata",
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            !text.chars().any(|c| matches!(
                c,
                '\u{85}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202e}'
                    | '\u{2066}'
                    | '\u{2069}'
                    | '\u{061c}'
                    | '\u{200e}'
                    | '\u{200f}'
            )),
            "raw line/bidi control in {format}"
        );
        assert!(!text.contains("<script>"));
        if format == "json" {
            let view: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(view["controlled_values"][0]["baseline"], attack);
            assert_eq!(view["artifacts"][0]["logical_path"], attack);
            assert_eq!(
                view["artifacts"][0]["package_metadata"]["metadata_version"],
                attack
            );
        }
    }
    let destination = root.path().join("local-\u{202e}-bundle");
    let preview = p03_preview(
        &input,
        &destination,
        &["controlled-values", "artifact-paths", "package-metadata"],
    );
    assert!(
        preview["destination"]
            .as_str()
            .unwrap()
            .contains('\u{202e}'),
        "local preview retains exact escaped destination"
    );
    let output = p03_command()
        .arg("export")
        .arg(&input)
        .arg("--output")
        .arg(&destination)
        .args([
            "--allow-field",
            "controlled-values",
            "--allow-field",
            "artifact-paths",
            "--allow-field",
            "package-metadata",
            "--approve",
        ])
        .arg(preview["approval_sha256"].as_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());
    for name in ["summary.json", "summary.md", "manifest.json"] {
        assert!(
            !String::from_utf8(std::fs::read(destination.join(name)).unwrap())
                .unwrap()
                .contains('\u{202e}')
        );
    }
}

#[test]
fn export_rejects_aggregate_output_over_limit() {
    let root = tempfile::tempdir().unwrap();
    let input = p03_input(root.path());
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&input).unwrap()).unwrap();
    let mut intervention = value["interventions"][0].clone();
    for key in ["variable", "baseline_value", "variant_value"] {
        intervention["intervention"][key] = "x".repeat(768).into();
    }
    value["interventions"] = serde_json::Value::Array(vec![intervention; 40]);
    std::fs::write(&input, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut total = 0;
    for format in ["json", "markdown"] {
        let output = p03_command()
            .arg("summary")
            .arg(&input)
            .args(["--allow-field", "controlled-values", "--format", format])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "each member must be otherwise valid: {output:?}"
        );
        assert!(output.stdout.len() <= 128 * 1024);
        total += output.stdout.len();
    }
    assert!(
        total > 192 * 1024,
        "fixture must exceed aggregate budget: {total}"
    );
    let destination = root.path().join("too-large");
    let output = p03_command()
        .arg("export")
        .arg(&input)
        .arg("--output")
        .arg(&destination)
        .args(["--allow-field", "controlled-values"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(5),
        "aggregate bound missing; size {total}"
    );
    assert!(output.stdout.is_empty());
    assert!(!destination.exists());
}

#[test]
fn derived_output_rejects_unwritable_and_closed_stdout() {
    let root = tempfile::tempdir().unwrap();
    let input = p03_input(root.path());
    let destination = root.path().join("preview-only");
    let sentinel = root.path().join("stdout-sentinel");
    std::fs::write(&sentinel, b"unchanged").unwrap();
    for args in [
        vec!["summary", "--format", "json"],
        vec!["summary", "--format", "markdown"],
        vec!["export", "--output", destination.to_str().unwrap()],
    ] {
        let output = p03_command()
            .args(&args)
            .arg(&input)
            .stdout(std::process::Stdio::from(
                std::fs::File::open(&sentinel).unwrap(),
            ))
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(5),
            "read-only output: {output:?}"
        );
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"unchanged");
        // Actual inherited closed stdout, not a writer mock. On Windows this
        // uses the MSYS sh from the developer environment, not Linux qualification.
        let binary = env!("CARGO_BIN_EXE_reprobisect").replace('\\', "/");
        let output = Command::new("sh")
            .args(["-c", "exec \"$@\" 1>&-", "p03-closed-stdout"])
            .arg(binary)
            .args(&args)
            .arg(&input)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(5),
            "closed descriptor must fail: {output:?}"
        );
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
        assert!(!destination.exists());
    }
}

fn p03_approved_export(
    input: &std::path::Path,
    destination: &std::path::Path,
    fields: &[&str],
    approval: &str,
) -> std::process::Output {
    let mut command = p03_command();
    command
        .arg("export")
        .arg(input)
        .arg("--output")
        .arg(destination)
        .arg("--approve")
        .arg(approval);
    for field in fields {
        command.args(["--allow-field", field]);
    }
    command.output().unwrap()
}

#[test]
fn p03_regression_exact_approval_and_create_only_bundle() {
    let root = tempfile::tempdir().unwrap();
    let input = p03_input(root.path());
    let original = std::fs::read(&input).unwrap();
    let destination = root.path().join("approved-bundle");
    let preview = p03_preview(&input, &destination, &[]);
    let approval = preview["approval_sha256"].as_str().unwrap();
    let wrong = p03_approved_export(&input, &destination, &[], &"0".repeat(64));
    assert_eq!(wrong.status.code(), Some(5));
    assert!(wrong.stdout.is_empty());
    assert!(!destination.exists());
    // Change only omitted data or whitespace: raw identity must still invalidate consent.
    let mut excluded_change: serde_json::Value = serde_json::from_slice(&original).unwrap();
    excluded_change["notes"] = serde_json::json!(["DIFFERENT_PRIVATE_NOTE"]);
    for changed in [
        serde_json::to_vec(&excluded_change).unwrap(),
        [original.as_slice(), b" \n"].concat(),
    ] {
        std::fs::write(&input, changed).unwrap();
        let stale = p03_approved_export(&input, &destination, &[], approval);
        assert_eq!(stale.status.code(), Some(5));
        assert!(stale.stdout.is_empty());
        assert!(!destination.exists());
    }
    std::fs::write(&input, &original).unwrap();
    let other_input = root.path().join("other.json");
    std::fs::write(&other_input, serde_json::to_vec(&excluded_change).unwrap()).unwrap();
    let stale = p03_approved_export(&other_input, &destination, &[], approval);
    assert_eq!(stale.status.code(), Some(5));
    assert!(!destination.exists());
    for fields in [
        &["controlled-values"][..],
        &["artifact-paths"][..],
        &["package-metadata"][..],
    ] {
        let stale = p03_approved_export(&input, &destination, fields, approval);
        assert_eq!(stale.status.code(), Some(5));
        assert!(stale.stdout.is_empty());
        assert!(!destination.exists());
    }
    let other_destination = root.path().join("not-approved");
    let stale = p03_approved_export(&input, &other_destination, &[], approval);
    assert_eq!(stale.status.code(), Some(5));
    assert!(stale.stdout.is_empty());
    assert!(!other_destination.exists());
    let accepted = p03_approved_export(&input, &destination, &[], approval);
    assert!(accepted.status.success(), "{accepted:?}");
    let before: Vec<_> = preview["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            let path = destination.join(m["name"].as_str().unwrap());
            (path.clone(), std::fs::read(path).unwrap())
        })
        .collect();
    let replacement = p03_approved_export(&input, &destination, &[], approval);
    assert_eq!(replacement.status.code(), Some(5));
    assert!(replacement.stdout.is_empty());
    for (path, bytes) in before {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    assert_eq!(std::fs::read(&input).unwrap(), original);
    let file_destination = root.path().join("existing-file");
    std::fs::write(&file_destination, b"sentinel").unwrap();
    let approval = p03_preview(&input, &file_destination, &[]);
    let rejected = p03_approved_export(
        &input,
        &file_destination,
        &[],
        approval["approval_sha256"].as_str().unwrap(),
    );
    assert_eq!(rejected.status.code(), Some(5));
    assert_eq!(std::fs::read(file_destination).unwrap(), b"sentinel");
}

#[test]
fn p03_regression_valid_approval_rejects_changed_link_ancestors() {
    let root = tempfile::tempdir().unwrap();
    let source_dir = root.path().join("source");
    std::fs::create_dir(&source_dir).unwrap();
    let input = p03_input(&source_dir);
    let original = std::fs::read(&input).unwrap();
    let outside = root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("sentinel"), b"untouched").unwrap();
    let destination = root.path().join("linked-leaf");
    let preview = p03_preview(&input, &destination, &[]);
    p03_directory_link(&outside, &destination);
    let rejected = p03_approved_export(
        &input,
        &destination,
        &[],
        preview["approval_sha256"].as_str().unwrap(),
    );
    assert_eq!(rejected.status.code(), Some(5));
    assert!(rejected.stdout.is_empty());
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
    assert_eq!(
        std::fs::read(outside.join("sentinel")).unwrap(),
        b"untouched"
    );
    let destination = root.path().join("source-swap-bundle");
    let preview = p03_preview(&input, &destination, &[]);
    let renamed = root.path().join("renamed-source");
    std::fs::rename(&source_dir, &renamed).unwrap();
    p03_directory_link(&renamed, &source_dir);
    let rejected = p03_approved_export(
        &input,
        &destination,
        &[],
        preview["approval_sha256"].as_str().unwrap(),
    );
    assert_eq!(rejected.status.code(), Some(5));
    assert!(rejected.stdout.is_empty());
    assert!(!destination.exists());
    assert_eq!(std::fs::read(renamed.join("input.json")).unwrap(), original);
    for format in ["json", "markdown"] {
        let output = p03_command()
            .arg("summary")
            .arg(&input)
            .args(["--format", format])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(5));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn p03_regression_input_field_record_and_render_bounds() {
    let root = tempfile::tempdir().unwrap();
    let input = p03_input(root.path());
    let original = std::fs::read(&input).unwrap();
    let destination = root.path().join("no-oversize-bundle");
    let mut large = original.clone();
    large.resize(4 * 1024 * 1024 + 1, b' ');
    std::fs::write(&input, &large).unwrap();
    for operation in ["summary", "export"] {
        let mut command = p03_command();
        command.arg(operation).arg(&input);
        if operation == "export" {
            command.arg("--output").arg(&destination);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(5));
        assert!(output.stdout.is_empty());
        assert!(!destination.exists());
        assert_eq!(std::fs::read(&input).unwrap(), large);
    }
    let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    for (field, pointer) in [
        (
            "controlled-values",
            "/interventions/0/intervention/baseline_value",
        ),
        ("artifact-paths", "/runs/0/artifacts/0/logical_path"),
        (
            "package-metadata",
            "/runs/0/artifacts/0/semantic_metadata/attributes/metadata_version",
        ),
    ] {
        let mut changed = value.clone();
        if field == "package-metadata" {
            changed["runs"][0]["artifacts"][0]["semantic_metadata"]["attributes"]["metadata_version"] =
                "short".into();
        }
        *changed.pointer_mut(pointer).unwrap() = "x".repeat(1025).into();
        std::fs::write(&input, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(
            p03_command()
                .arg("summary")
                .arg(&input)
                .output()
                .unwrap()
                .status
                .success(),
            "unselected long private fields are omitted, not copied"
        );
        for format in ["json", "markdown"] {
            let output = p03_command()
                .arg("summary")
                .arg(&input)
                .args(["--format", format, "--allow-field", field])
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(5));
            assert!(output.stdout.is_empty());
        }
    }
    let mut records = value.clone();
    records["runs"] = serde_json::Value::Array(vec![value["runs"][0].clone(); 129]);
    std::fs::write(&input, serde_json::to_vec(&records).unwrap()).unwrap();
    let output = p03_command().arg("summary").arg(&input).output().unwrap();
    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    let mut expanded = value.clone();
    let mut intervention = value["interventions"][0].clone();
    for key in ["variable", "baseline_value", "variant_value"] {
        intervention["intervention"][key] = "x".repeat(1024).into();
    }
    expanded["interventions"] = serde_json::Value::Array(vec![intervention; 80]);
    std::fs::write(&input, serde_json::to_vec(&expanded).unwrap()).unwrap();
    for format in ["json", "markdown"] {
        let output = p03_command()
            .arg("summary")
            .arg(&input)
            .args(["--format", format, "--allow-field", "controlled-values"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(5));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn p03_regression_sidecar_uses_complete_authoritative_context_and_rejects_unsafe_output() {
    let project = tempfile::tempdir().unwrap();
    let sidecar = project.path().join("complete.json");
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[execution]\ntotal_timeout_seconds=60\n[experiments]\ncontrol_runs=2\n[experiments.dimensions]\nbuild_path=false\nsource_date_epoch=false\ntimezone=false\nlocale=false\nhostname=false\n").unwrap();
    let output = double_command(project.path(), "stable")
        .arg("diagnose")
        .arg(project.path())
        .args(["--ci", "--summary-output"])
        .arg(&sidecar)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "CLI double, not native runtime qualification: {output:?}"
    );
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let view: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&sidecar).unwrap()).unwrap();
    assert_eq!(envelope["operation"], "diagnose");
    assert_eq!(envelope["coverage"]["complete"], true);
    assert_eq!(view["coverage"]["authoritative"], envelope["coverage"]);
    assert_eq!(view["completion"], "completed");
    assert_eq!(view["execution_completed"], true);
    assert_eq!(view["execution_reason"], "completed");
    assert_eq!(view["diagnostic_status"], "reproducible");
    assert_eq!(view["original_input_sha256"], envelope["report"]["sha256"]);
    assert!(!String::from_utf8_lossy(&std::fs::read(&sidecar).unwrap()).contains("actual stdout"));
    let isolated = p03_command()
        .arg("summary")
        .arg(
            project
                .path()
                .join(envelope["report"]["path"].as_str().unwrap()),
        )
        .output()
        .unwrap();
    let isolated: serde_json::Value = serde_json::from_slice(&isolated.stdout).unwrap();
    assert_eq!(
        isolated["coverage"]["status"], "unknown",
        "standalone input must not fabricate current-run planned scope"
    );
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[execution]\nmax_dispatches=0\n").unwrap();
    let outside = project.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let alias = project.path().join("alias");
    p03_directory_link(&outside, &alias);
    for path in [
        alias.join("summary.json"),
        outside.join("..").join("escape.json"),
        project.path().join("missing-parent").join("summary.json"),
    ] {
        let output = p03_command()
            .arg("check")
            .arg(project.path())
            .args(["--ci", "--summary-output"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(5));
        let failure: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(failure["completion"]["reason"], "operational_error");
        assert!(failure["diagnostic_status"].is_null());
        assert!(!outside.join("summary.json").exists());
        assert!(!project.path().join("escape.json").exists());
        assert!(!project.path().join("missing-parent").exists());
    }
    std::fs::write(project.path().join(".reprobisect.toml"), "not valid TOML").unwrap();
    let absent = project.path().join("no-invented-summary.json");
    let output = p03_command()
        .arg("check")
        .arg(project.path())
        .args(["--ci", "--summary-output"])
        .arg(&absent)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    let failure: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(failure["diagnostic_status"].is_null());
    assert!(failure["coverage"].is_null());
    assert!(!absent.exists());
}

#[test]
fn derived_output_failure_keeps_exit_five_with_unwritable_stderr() {
    let root = tempfile::tempdir().unwrap();
    let input = p03_input(root.path());
    let destination = root.path().join("no-bundle");
    let sentinel = root.path().join("stderr-sentinel");
    std::fs::write(&sentinel, b"unchanged").unwrap();
    for args in [
        vec!["summary", "--format", "json"],
        vec!["summary", "--format", "markdown"],
        vec!["export", "--output", destination.to_str().unwrap()],
    ] {
        let success = p03_command()
            .args(&args)
            .arg(&input)
            .stderr(Stdio::from(std::fs::File::open(&sentinel).unwrap()))
            .output()
            .unwrap();
        assert!(success.status.success());
        assert!(!success.stdout.is_empty());
        let failure = p03_command()
            .args(&args)
            .arg(&input)
            .stdout(Stdio::from(std::fs::File::open(&sentinel).unwrap()))
            .stderr(Stdio::from(std::fs::File::open(&sentinel).unwrap()))
            .output()
            .unwrap();
        assert_eq!(
            failure.status.code(),
            Some(5),
            "stderr must not turn a derived transport failure into a panic: {failure:?}"
        );
        assert!(failure.stdout.is_empty());
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"unchanged");
        assert!(!destination.exists());
    }
}

fn p03_approval_for_destination(preview: &serde_json::Value, destination: &str) -> String {
    // Reconstruct the public binding independently. Verify against a valid
    // preview first so negative path tests cannot pass only on wrong approval.
    #[derive(serde::Serialize)]
    struct Descriptor<'a> {
        name: &'a str,
        size_bytes: u64,
        sha256: &'a str,
    }
    #[derive(serde::Serialize)]
    struct Manifest<'a> {
        policy_version: &'a str,
        renderer_version: &'a str,
        original_input_sha256: &'a str,
        allowed_fields: &'a serde_json::Value,
        members: Vec<Descriptor<'a>>,
    }
    #[derive(serde::Serialize)]
    struct Approval<'a> {
        manifest: Manifest<'a>,
        destination: &'a str,
    }
    let manifest = Manifest {
        policy_version: preview["policy_version"].as_str().unwrap(),
        renderer_version: preview["renderer_version"].as_str().unwrap(),
        original_input_sha256: preview["original_input_sha256"].as_str().unwrap(),
        allowed_fields: &preview["allowed_fields"],
        members: preview["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|member| Descriptor {
                name: member["name"].as_str().unwrap(),
                size_bytes: member["size_bytes"].as_u64().unwrap(),
                sha256: member["sha256"].as_str().unwrap(),
            })
            .collect(),
    };
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(
        serde_json::to_vec(&Approval {
            manifest,
            destination,
        })
        .unwrap(),
    ))
}

#[test]
fn p03_regression_device_names_and_prefixes_fail_before_preview_or_commit() {
    let root = tempfile::tempdir().unwrap();
    let input = p03_input(root.path());
    let preview = p03_preview(&input, &root.path().join("safe"), &[]);
    assert_eq!(
        p03_approval_for_destination(&preview, preview["destination"].as_str().unwrap()),
        preview["approval_sha256"]
    );
    for name in [
        "CON",
        "con.json",
        "NUL.log",
        "AUX",
        "PRN",
        "COM1",
        "LPT9",
        "COM¹",
        "LPT²",
        "CONIN$",
        "CONOUT$",
        "trailing.",
        "trailing ",
        "stream:ads",
        "wild*card",
        "wild?card",
        "pipe|name",
    ] {
        let output_path = root.path().join(name);
        let canonical_destination = std::fs::canonicalize(root.path()).unwrap().join(name);
        let approval =
            p03_approval_for_destination(&preview, canonical_destination.to_str().unwrap());
        for approved in [false, true] {
            let mut command = p03_command();
            command
                .arg("export")
                .arg(&input)
                .arg("--output")
                .arg(&output_path);
            if approved {
                command.arg("--approve").arg(&approval);
            }
            let output = command.output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(5),
                "unsafe name must fail even with otherwise matching content approval: {name}/{approved}: {output:?}"
            );
            assert!(output.stdout.is_empty());
        }
    }
    #[cfg(windows)]
    for path in [
        r"C:relative-bundle",
        r"\unexpected-root",
        r"\\?\C:\unexpected-bundle",
        r"\\.\NUL",
        r"\\?\GLOBALROOT\Device\HarddiskVolume1\unexpected-bundle",
    ] {
        let output = p03_command()
            .arg("export")
            .arg(&input)
            .args(["--output", path])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(5),
            "unexpected prefix must fail before preview: {path}: {output:?}"
        );
        assert!(output.stdout.is_empty());
    }
    assert_eq!(
        std::fs::read_dir(root.path()).unwrap().count(),
        1,
        "no unsafe leaf created"
    );
}

#[test]
#[cfg(unix)]
fn p03_unix_fifo_source_is_rejected_without_blocking_open() {
    let root = tempfile::tempdir().unwrap();
    let fifo = root.path().join("source-fifo");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let mut child = p03_command()
        .arg("summary")
        .arg(&fifo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if std::time::Instant::now() >= until {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "nonregular source blocked in open instead of fail-closed inspection: {output:?}"
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
}

#[test]
#[cfg(target_os = "linux")]
fn p03_linux_sigint_sidecar_keeps_precise_incomplete_state() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\ntimeout_seconds=20\n[execution]\ntotal_timeout_seconds=60\n").unwrap();
    let sidecar = project.path().join("cancelled-summary.json");
    let child = double_command(project.path(), "timeout")
        .arg("check")
        .arg(project.path())
        .args(["--ci", "--ci-policy", "report-only", "--summary-output"])
        .arg(&sidecar)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !project.path().join("build-name").exists() {
        assert!(
            std::time::Instant::now() < until,
            "active build launch was not observed"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let output = bounded_wait(child);
    assert_eq!(output.status.code(), Some(5));
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let view: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&sidecar).unwrap()).unwrap();
    assert_eq!(view["diagnostic_status"], envelope["diagnostic_status"]);
    assert_eq!(view["coverage"]["authoritative"], envelope["coverage"]);
    assert_eq!(view["coverage"]["complete"], false);
    assert_eq!(view["execution_completed"], false);
    assert_eq!(view["execution_reason"], "cancelled");
    assert_eq!(view["original_input_sha256"], envelope["report"]["sha256"]);
}

#[test]
fn p03_json_neutralizes_embedded_legacy_ci_commands() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let input = p03_input(root.path());
    let marker = "##vso[task.logissue type=error;]P03_INERT_##[error]P03_INERT";
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&input).unwrap()).unwrap();
    value["interventions"][0]["intervention"]["baseline_value"] = marker.into();
    value["interventions"][0]["intervention"]["variant_value"] = marker.into();
    std::fs::write(&input, serde_json::to_vec(&value).unwrap()).unwrap();
    let original = std::fs::read(&input).unwrap();
    let summary = p03_command()
        .arg("summary")
        .arg(&input)
        .args(["--format", "json", "--allow-field", "controlled-values"])
        .output()
        .unwrap();
    assert!(
        summary.status.success(),
        "typed summary fixture failed: {summary:?}"
    );
    let view: serde_json::Value = serde_json::from_slice(&summary.stdout).unwrap();
    assert_eq!(view["controlled_values"][0]["baseline"], marker);
    assert_eq!(view["controlled_values"][0]["variant"], marker);
    assert_eq!(
        view["original_input_sha256"],
        hex::encode(Sha256::digest(&original))
    );
    let text = std::str::from_utf8(&summary.stdout).unwrap();
    for prefix in ["##vso[", "##["] {
        assert!(
            !text.contains(prefix),
            "quoted summary values retain an active CI delimiter: {prefix:?}"
        );
    }
    for (index, fields) in [&[][..], &["controlled-values"][..]]
        .into_iter()
        .enumerate()
    {
        let leaf = format!("bundle-{index}-{marker}");
        let destination = root.path().join(&leaf);
        let mut command = p03_command();
        command
            .arg("export")
            .arg(&input)
            .arg("--output")
            .arg(&destination);
        for field in fields {
            command.args(["--allow-field", field]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "legal marker destination preview failed: {output:?}"
        );
        assert!(!destination.exists());
        let text = std::str::from_utf8(&output.stdout).unwrap();
        let preview: serde_json::Value = serde_json::from_str(text).unwrap();
        assert!(preview["destination"].as_str().unwrap().ends_with(&leaf));
        assert_eq!(
            preview["original_input_sha256"],
            hex::encode(Sha256::digest(&original))
        );
        for prefix in ["##vso[", "##["] {
            assert!(
                !text.contains(prefix),
                "default or selected preview retains an active CI delimiter: {prefix:?}"
            );
        }
        let mut approved = p03_command();
        approved
            .arg("export")
            .arg(&input)
            .arg("--output")
            .arg(&destination)
            .arg("--approve")
            .arg(preview["approval_sha256"].as_str().unwrap());
        for field in fields {
            approved.args(["--allow-field", field]);
        }
        let exported = approved.output().unwrap();
        assert!(
            exported.status.success(),
            "exact approved payload rejected: {exported:?}"
        );
        for member in preview["members"].as_array().unwrap() {
            let bytes = std::fs::read(destination.join(member["name"].as_str().unwrap())).unwrap();
            assert_eq!(bytes, member["bytes"].as_str().unwrap().as_bytes());
            assert_eq!(member["sha256"], hex::encode(Sha256::digest(&bytes)));
            assert_eq!(member["size_bytes"].as_u64().unwrap(), bytes.len() as u64);
        }
        assert_eq!(std::fs::read(&input).unwrap(), original);
    }
}

#[test]
fn p03_non_ci_sidecar_failure_keeps_exit_five_with_unwritable_stderr() {
    for operation in ["check", "diagnose"] {
        for format in ["text", "json"] {
            for ci in [false, true] {
                for read_only_stderr in [false, true] {
                    let project = tempfile::tempdir().unwrap();
                    std::fs::write(
                        project.path().join(".reprobisect.toml"),
                        "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[execution]\nmax_dispatches=0\n",
                    ).unwrap();
                    let sidecar = project.path().join("existing-summary.json");
                    let stderr = project.path().join("stderr-sentinel");
                    std::fs::write(&sidecar, b"do not replace").unwrap();
                    std::fs::write(&stderr, b"unchanged").unwrap();
                    let mut command = p03_command();
                    command
                        .arg(operation)
                        .arg(project.path())
                        .args(["--format", format, "--summary-output"])
                        .arg(&sidecar);
                    if ci {
                        command.arg("--ci");
                    }
                    if read_only_stderr {
                        command.stderr(Stdio::from(std::fs::File::open(&stderr).unwrap()));
                    }
                    let output = command.output().unwrap();
                    assert_eq!(
                        output.status.code(),
                        Some(5),
                        "opt-in create-only sidecar failure must not panic on stderr: operation={operation} format={format} ci={ci} read_only_stderr={read_only_stderr}; {output:?}",
                    );
                    assert_eq!(std::fs::read(&sidecar).unwrap(), b"do not replace");
                    assert_eq!(std::fs::read(&stderr).unwrap(), b"unchanged");
                    let receipts: Vec<_> =
                        std::fs::read_dir(project.path().join(".reprobisect/executions"))
                            .unwrap()
                            .map(|entry| entry.unwrap().path())
                            .collect();
                    assert_eq!(receipts.len(), 1);
                    let receipt: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(&receipts[0]).unwrap()).unwrap();
                    assert_eq!(receipt["charged_dispatches"], 0);
                    assert_eq!(receipt["completed"], false);
                    assert_eq!(receipt["completion_reason"], "budget_exhausted");
                    if ci {
                        let envelope: serde_json::Value =
                            serde_json::from_slice(&output.stdout).unwrap();
                        assert_eq!(envelope["completion"]["reason"], "operational_error");
                        assert_eq!(envelope["completion"]["completed"], false);
                    }
                }
            }
        }
    }
}
