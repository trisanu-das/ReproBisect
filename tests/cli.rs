use std::process::{Command, Stdio};

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
