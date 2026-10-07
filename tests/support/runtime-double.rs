// Adversarial CLI runtime double, not Docker/Podman qualification.
use std::{
    env, fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::Duration,
};
fn main() {
    let a: Vec<String> = env::args().skip(1).collect();
    let root = PathBuf::from(env::var("PROBE_ROOT").unwrap());
    let mode = env::var("PROBE_MODE").unwrap();
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("runtime.log"))
        .unwrap();
    let arguments = a.join(" | ");
    if mode == "podman-stop-grace" || mode == "failed-build-spawn" {
        // This regression counts dispatches, not lines in the probe's shell script.
        writeln!(
            log,
            "{}",
            arguments.replace('\n', "\\n").replace('\r', "\\r")
        )
        .unwrap();
    } else {
        writeln!(log, "{arguments}").unwrap();
    }
    let flag = |key: &str| {
        a.iter()
            .position(|v| v == key)
            .and_then(|i| a.get(i + 1))
            .cloned()
            .unwrap_or_default()
    };
    match a.first().map(String::as_str).unwrap_or("") {
        "version" | "info" => println!("runtime double"),
        "image" => {
            if mode == "failed-probe-spawn" {
                let executable = env::current_exe().unwrap();
                fs::rename(&executable, root.join("retired-runtime.exe")).unwrap();
                // EACCES searches later PATH entries; ENOEXEC may spawn a shell.
                // A self-referential symlink instead makes POSIX exec fail with
                // ELOOP, which stops PATH search even when real Docker is later.
                #[cfg(unix)]
                std::os::unix::fs::symlink(&executable, &executable).unwrap();
                #[cfg(not(unix))]
                fs::write(&executable, b"not an executable").unwrap();
            }
            println!("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        }
        "run" => {
            let name = flag("--name");
            let label = flag("--label").replace("reprobisect.operation=", "");
            let cid = flag("--cidfile");
            let id: String = name
                .bytes()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                .chars()
                .rev()
                .take(64)
                .collect();
            if !name.contains("probe") && mode == "collision" {
                fs::write(root.join("unrelated-container"), &name).unwrap();
                eprintln!("Conflict: unrelated name already exists");
                std::process::exit(125);
            }
            if !name.contains("probe") && matches!(mode.as_str(), "race" | "ack-late") {
                Command::new(env::current_exe().unwrap())
                    .args(["late-create", &id, &label, &cid, &name])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap();
            } else {
                fs::write(root.join(&id), format!("{label}\n{name}")).unwrap();
                if !cid.is_empty() {
                    fs::write(&cid, &id).unwrap();
                }
            }
            if name.contains("probe") {
                if mode == "probe-open-pipes" {
                    Command::new(env::current_exe().unwrap())
                        .arg("hold-pipes")
                        .spawn()
                        .unwrap();
                }
                println!("cc\tdouble");
                return;
            }
            fs::write(root.join("build-name"), &name).unwrap();
            if matches!(
                mode.as_str(),
                "failed-build-spawn" | "fix-verify-stable" | "fix-verify-timeout"
            ) {
                let count = fs::read_to_string(root.join("build-count"))
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
                    + 1;
                fs::write(root.join("build-count"), count.to_string()).unwrap();
            }
            let mount = flag("--mount");
            let volume = flag("--volume");
            let src = mount
                .strip_prefix("type=bind,src=")
                .and_then(|v| v.split(",dst=").next())
                .map(str::to_string)
                .or_else(|| volume.strip_suffix(":/workspace").map(str::to_string));
            if let Some(src) = src {
                let workspace = PathBuf::from(src);
                let fix = mode.starts_with("fix-verify-");
                let count = if fix {
                    fs::read_to_string(root.join("build-count"))
                        .ok()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0)
                } else {
                    0
                };
                let bytes = if fix && (count == 3 || count == 4) {
                    b"changed diagnosis artifact".as_slice()
                } else {
                    b"actual interrupted artifact".as_slice()
                };
                fs::write(workspace.join("out"), bytes).unwrap();
                if fix && count >= 6 {
                    let patched_makefile = fs::read_to_string(workspace.join("Makefile")).unwrap();
                    assert!(patched_makefile.contains("# ReproBisect candidate:"));
                    fs::write(root.join("patched-makefile-observed"), patched_makefile).unwrap();
                }
            }
            println!("actual stdout before interrupt");
            std::io::stdout().flush().unwrap();
            if mode == "baseline-late" {
                let count = fs::read_to_string(root.join("build-count"))
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
                    + 1;
                fs::write(root.join("build-count"), count.to_string()).unwrap();
                if count == 1 {
                    return;
                }
            }
            if mode == "ordinary-build-failed" {
                eprintln!("ordinary executed command failure");
                std::process::exit(1);
            }
            if mode == "successful-parent-pipes" {
                Command::new(env::current_exe().unwrap())
                    .arg("hold-pipes")
                    .spawn()
                    .unwrap();
                return;
            }
            if mode == "fix-verify-timeout"
                && fs::read_to_string(root.join("build-count")).ok().as_deref() == Some("6")
            {
                thread::sleep(Duration::from_secs(10));
            }
            if matches!(
                mode.as_str(),
                "stable"
                    | "cleanupfail"
                    | "probe-open-pipes"
                    | "failed-build-spawn"
                    | "fix-verify-stable"
                    | "fix-verify-timeout"
            ) {
                return;
            }
            if mode == "pipes" {
                Command::new(env::current_exe().unwrap())
                    .arg("hold-pipes")
                    .spawn()
                    .unwrap();
            }
            thread::sleep(Duration::from_secs(10));
        }
        "late-create" => {
            thread::sleep(Duration::from_millis(if mode == "race" {
                5500
            } else {
                1500
            }));
            fs::write(root.join(&a[1]), format!("{}\n{}", a[2], a[4])).unwrap();
            fs::write(root.join("late-owned-container"), &a[1]).unwrap();
            if mode != "race" {
                let _ = fs::write(&a[3], &a[1]);
            }
        }
        "hold-pipes" => thread::sleep(Duration::from_secs(10)),
        "container" => {
            let id = a.last().unwrap();
            if let Ok(raw) = fs::read_to_string(root.join(id)) {
                let label = raw.lines().next().unwrap();
                println!(
                    "{{\"Id\":\"{id}\",\"Config\":{{\"Labels\":{{\"reprobisect.operation\":\"{}\"}}}}}}",
                    if mode == "identity-mismatch" {
                        "unrelated"
                    } else {
                        label
                    }
                );
            } else {
                std::process::exit(1);
            }
        }
        "rm" => {
            let id = a.last().unwrap();
            assert_eq!(id.len(), 64, "destructive target must be exact ID");
            if mode == "cleanupfail" {
                std::process::exit(1);
            }
            if mode == "podman-stop-grace" {
                let executable = env::current_exe().unwrap();
                let podman = executable.file_stem().unwrap() == "podman";
                if !podman && a.iter().any(|v| v == "--time") {
                    eprintln!("Docker rm does not support Podman's --time");
                    std::process::exit(125);
                }
                let active_build = fs::read_to_string(root.join(id))
                    .unwrap()
                    .lines()
                    .nth(1)
                    .is_some_and(|name| name.starts_with("reprobisect-build-"));
                if podman && active_build && flag("--time") != "0" {
                    // Model Podman 4.9.3's default stop grace exceeding the
                    // private three-second cleanup deadline, not native OCI.
                    fs::write(root.join("default-stop-grace"), id).unwrap();
                    thread::sleep(Duration::from_secs(10));
                }
            }
            let _ = fs::remove_file(root.join(id));
            let _ = fs::remove_file(root.join("late-owned-container"));
            fs::write(root.join("reused-name-unrelated"), "survives").unwrap();
        }
        "ps" => {
            let id = flag("--filter").replace("id=", "");
            if root.join(&id).is_file() {
                println!("{id}");
            }
            if mode == "failed-build-spawn"
                && fs::read_to_string(root.join("build-count")).ok().as_deref() == Some("2")
            {
                let executable = env::current_exe().unwrap();
                fs::rename(&executable, root.join("retired-postbaseline-runtime.exe")).unwrap();
                // Fatal ELOOP forbids PATH fallback or a shell on POSIX.
                #[cfg(unix)]
                std::os::unix::fs::symlink(&executable, &executable).unwrap();
                #[cfg(not(unix))]
                fs::write(&executable, b"invalid native executable").unwrap();
                fs::write(
                    root.join("postbaseline-spawn-armed"),
                    "two baselines cleaned",
                )
                .unwrap();
            }
        }
        _ => {}
    }
}
