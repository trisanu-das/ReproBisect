use std::process::Command;

#[test]
#[ignore = "requires built binary and Docker; exercised in CI integration job later"]
fn check_help_smoke_test() {
    let status = Command::new(env!("CARGO_BIN_EXE_reprobisect"))
        .args(["check", "--help"])
        .status()
        .unwrap();
    assert!(status.success());
}
