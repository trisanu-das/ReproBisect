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
    writeln!(log, "{}", a.join(" | ")).unwrap();
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
            let mount = flag("--mount");
            let volume = flag("--volume");
            let src = mount
                .strip_prefix("type=bind,src=")
                .and_then(|v| v.split(",dst=").next())
                .map(str::to_string)
                .or_else(|| volume.strip_suffix(":/workspace").map(str::to_string));
            if let Some(src) = src {
                fs::write(
                    PathBuf::from(src).join("out"),
                    b"actual interrupted artifact",
                )
                .unwrap();
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
            if mode == "stable" || mode == "cleanupfail" || mode == "probe-open-pipes" {
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
            let _ = fs::remove_file(root.join(id));
            let _ = fs::remove_file(root.join("late-owned-container"));
            fs::write(root.join("reused-name-unrelated"), "survives").unwrap();
        }
        "ps" => {
            let id = flag("--filter").replace("id=", "");
            if root.join(&id).is_file() {
                println!("{id}");
            }
        }
        _ => {}
    }
}
