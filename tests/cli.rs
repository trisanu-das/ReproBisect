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
    std::fs::copy(
        runtime_double().join(format!("docker{}", std::env::consts::EXE_SUFFIX)),
        runtime
            .path()
            .join(format!("docker{}", std::env::consts::EXE_SUFFIX)),
    )
    .unwrap();
    std::fs::write(project.path().join(".reprobisect.toml"), "[build]\nimage='unused'\ncommand=['true']\noutputs=['out']\n[execution]\ntotal_timeout_seconds=30\n").unwrap();
    let output = double_command(project.path(), "failed-probe-spawn")
        .env(
            "PATH",
            std::env::join_paths(
                std::iter::once(runtime.path().to_path_buf())
                    .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
            )
            .unwrap(),
        )
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
    assert_eq!(log.lines().count(), 2, "only info and image inspect: {log}");
    let owned = receipt["owned_containers"].as_array().unwrap();
    assert_eq!(owned[0]["creation_outcome"], "not_dispatched", "{receipt}");
    assert_eq!(owned[0]["cleanup_verified"], true);
    assert_eq!(
        owned.len(),
        1,
        "no additional reservations after infrastructure spawn failed"
    );
    assert_eq!(
        receipt["charged_dispatches"], 8,
        "git provenance + version + build + image + 3 cleanup + failed spawn remain charged, no refunds"
    );
    assert_eq!(receipt["completion_reason"], "operational_error");
    assert!(
        receipt["elapsed_millis"].as_u64().unwrap() < 2500,
        "no CID reconciliation for failed spawn: {receipt}"
    );
    assert_eq!(output.status.code(), Some(5));
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["completion"]["reason"], "operational_error");
    assert_eq!(envelope["policy"]["passed"], false);
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
        assert_eq!(output.status.code(), Some(5), "{output:?}");
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
        assert_eq!(receipt["completion_reason"], phase);
        assert_eq!(receipt["completed"], false);
        assert!(receipt["charged_dispatches"].as_u64().unwrap() > 0);
        assert!(
            receipt["owned_containers"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["cleanup_verified"] == true)
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
