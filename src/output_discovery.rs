use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::UNIX_EPOCH,
};

use anyhow::{Context, Result, bail};
use walkdir::{DirEntry, WalkDir};

use crate::{
    artifact::detect_type,
    model::{ArtifactType, RunnerBackend, SourceCopyOrder},
    source::copy_source_tree,
};

const MAX_SCAN_FILES: usize = 100_000;
const MAX_CANDIDATES: usize = 12;
const FAILURE_OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateConfidence {
    High,
    Medium,
    Low,
}

impl CandidateConfidence {
    pub const fn label(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OutputCandidate {
    pub path: PathBuf,
    pub kind: String,
    pub confidence: CandidateConfidence,
    pub size_bytes: u64,
    score: i32,
}

#[derive(Debug, Clone)]
pub struct OutputDiscoveryResult {
    pub candidates: Vec<OutputCandidate>,
    pub changed_file_count: usize,
    pub scan_truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    size_bytes: u64,
    modified_nanos: Option<u128>,
    mode: u32,
}

#[derive(Debug)]
struct Snapshot {
    files: BTreeMap<PathBuf, FileStamp>,
    truncated: bool,
}

pub fn discover_outputs(
    project: &Path,
    image: &str,
    command: &[String],
    runner: RunnerBackend,
) -> Result<OutputDiscoveryResult> {
    if command.is_empty() {
        bail!("cannot probe outputs with an empty build command");
    }

    let workspace = tempfile::Builder::new()
        .prefix("reprobisect-output-discovery-")
        .tempdir()
        .context("cannot create temporary output-discovery workspace")?;
    copy_source_tree(project, workspace.path(), SourceCopyOrder::Sorted)
        .context("cannot prepare temporary output-discovery workspace")?;

    let before = snapshot(workspace.path())?;
    run_build(workspace.path(), image, command, runner)?;
    let after = snapshot(workspace.path())?;

    let (mut candidates, changed_file_count) =
        candidates_from_snapshots(workspace.path(), &before, &after)?;
    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
    });
    candidates.truncate(MAX_CANDIDATES);

    Ok(OutputDiscoveryResult {
        candidates,
        changed_file_count,
        scan_truncated: before.truncated || after.truncated,
    })
}

fn run_build(
    workspace: &Path,
    image: &str,
    build_command: &[String],
    runner: RunnerBackend,
) -> Result<()> {
    let runtime = runner.executable();
    let mut command = Command::new(runtime);
    command
        .arg("run")
        .arg("--rm")
        .arg("--volume")
        .arg(format!("{}:/workspace", workspace.display()))
        .arg("--workdir")
        .arg("/workspace")
        .arg("--env")
        .arg("HOME=/tmp/reprobisect-home")
        .arg("--env")
        .arg("CARGO_HOME=/tmp/reprobisect-cargo")
        .arg("--env")
        .arg("GRADLE_USER_HOME=/tmp/reprobisect-gradle")
        .arg("--env")
        .arg("NPM_CONFIG_CACHE=/tmp/reprobisect-npm")
        .arg("--env")
        .arg("PIP_CACHE_DIR=/tmp/reprobisect-pip");

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(workspace)
            .with_context(|| format!("cannot stat {}", workspace.display()))?;
        command
            .arg("--user")
            .arg(format!("{}:{}", metadata.uid(), metadata.gid()));
    }

    command
        .arg(image)
        .args(build_command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().with_context(|| {
        format!(
            "cannot execute {} for output discovery; run reprobisect doctor to check runtime readiness",
            runner.display_name()
        )
    })?;
    let stdout = child
        .stdout
        .take()
        .context("output-discovery stdout pipe was not available")?;
    let stderr = child
        .stderr
        .take()
        .context("output-discovery stderr pipe was not available")?;
    let stdout_thread = thread::spawn(move || drain_tail(stdout));
    let stderr_thread = thread::spawn(move || drain_tail(stderr));

    let status = child
        .wait()
        .context("cannot wait for temporary output-discovery build")?;
    let stdout = join_capture(stdout_thread, "stdout")?;
    let stderr = join_capture(stderr_thread, "stderr")?;

    if !status.success() {
        bail!(
            "temporary discovery build failed with status {}{}{}",
            status,
            if stderr.is_empty() {
                String::new()
            } else {
                format!("\nstderr (tail):\n{stderr}")
            },
            if stdout.is_empty() {
                String::new()
            } else {
                format!("\nstdout (tail):\n{stdout}")
            }
        );
    }

    Ok(())
}

fn drain_tail<R: Read>(mut reader: R) -> std::io::Result<String> {
    let mut retained = Vec::with_capacity(FAILURE_OUTPUT_BYTES);
    let mut buffer = [0_u8; 16 * 1024];

    loop {
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if read == 0 {
            break;
        }

        if read >= FAILURE_OUTPUT_BYTES {
            retained.clear();
            retained.extend_from_slice(&buffer[read - FAILURE_OUTPUT_BYTES..read]);
            continue;
        }

        let overflow = retained
            .len()
            .saturating_add(read)
            .saturating_sub(FAILURE_OUTPUT_BYTES);
        if overflow > 0 {
            retained.drain(..overflow);
        }
        retained.extend_from_slice(&buffer[..read]);
    }

    Ok(String::from_utf8_lossy(&retained).trim().to_string())
}

fn join_capture(
    handle: thread::JoinHandle<std::io::Result<String>>,
    stream: &str,
) -> Result<String> {
    let captured = handle
        .join()
        .map_err(|_| anyhow::anyhow!("{stream} capture thread panicked"))?;
    captured.with_context(|| format!("cannot read output-discovery {stream}"))
}

fn snapshot(root: &Path) -> Result<Snapshot> {
    let mut files = BTreeMap::new();
    let mut truncated = false;

    let walker = WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(should_descend);

    for entry in walker {
        let entry = entry.with_context(|| format!("cannot scan {}", root.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        if files.len() >= MAX_SCAN_FILES {
            truncated = true;
            break;
        }

        let absolute = entry.path();
        let relative = absolute
            .strip_prefix(root)
            .expect("walk entry remains under discovery root")
            .to_path_buf();
        let metadata = fs::metadata(absolute)
            .with_context(|| format!("cannot stat discovery file {}", absolute.display()))?;
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_nanos());

        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode()
        };
        #[cfg(not(unix))]
        let mode = 0;

        files.insert(
            relative,
            FileStamp {
                size_bytes: metadata.len(),
                modified_nanos,
                mode,
            },
        );
    }

    Ok(Snapshot { files, truncated })
}

fn should_descend(entry: &DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    !matches!(
        name.as_ref(),
        ".git"
            | ".reprobisect"
            | "node_modules"
            | ".venv"
            | "venv"
            | "__pycache__"
            | ".pytest_cache"
            | ".mypy_cache"
            | ".idea"
            | ".vscode"
    )
}

fn candidates_from_snapshots(
    root: &Path,
    before: &Snapshot,
    after: &Snapshot,
) -> Result<(Vec<OutputCandidate>, usize)> {
    let mut changed_file_count = 0;
    let mut candidates = Vec::new();

    for (relative, stamp) in &after.files {
        if before.files.get(relative) == Some(stamp) {
            continue;
        }
        changed_file_count += 1;
        if let Some(candidate) = classify_candidate(root, relative, stamp)? {
            candidates.push(candidate);
        }
    }

    Ok((candidates, changed_file_count))
}

fn classify_candidate(
    root: &Path,
    relative: &Path,
    stamp: &FileStamp,
) -> Result<Option<OutputCandidate>> {
    if stamp.size_bytes == 0 || obvious_intermediate_extension(relative) {
        return Ok(None);
    }

    let absolute = root.join(relative);
    let detected = detect_type(&absolute).unwrap_or(ArtifactType::Generic);
    let lower = relative.to_string_lossy().to_ascii_lowercase();
    let extension = relative
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let (kind, mut score) = match detected {
        ArtifactType::Elf => {
            if extension == "so" || lower.contains(".so.") {
                ("ELF shared library".to_string(), 88)
            } else {
                ("ELF executable".to_string(), 95)
            }
        }
        ArtifactType::PeCoff => ("PE/COFF binary".to_string(), 92),
        ArtifactType::MachO => ("Mach-O binary".to_string(), 92),
        ArtifactType::Wasm => ("WebAssembly module".to_string(), 88),
        ArtifactType::Jar => ("JAR package".to_string(), 94),
        ArtifactType::PythonWheel => ("Python wheel".to_string(), 96),
        ArtifactType::Deb => ("DEB package".to_string(), 96),
        ArtifactType::OciImage => ("OCI image archive".to_string(), 94),
        ArtifactType::Zip => ("ZIP archive".to_string(), 80),
        ArtifactType::Tar => ("TAR archive".to_string(), 80),
        ArtifactType::Gzip => {
            if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
                ("compressed package archive".to_string(), 84)
            } else {
                ("gzip artifact".to_string(), 68)
            }
        }
        ArtifactType::Ar => ("static/archive library".to_string(), 82),
        ArtifactType::Generic => {
            if let Some(value) = generic_kind(relative, stamp) {
                value
            } else {
                return Ok(None);
            }
        }
    };

    if likely_final_directory(relative) {
        score += 10;
    }
    if likely_intermediate_path(relative) {
        score -= 35;
    }
    if relative
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.starts_with('.'))
    {
        score -= 20;
    }
    if score < 55 {
        return Ok(None);
    }

    let confidence = if score >= 90 {
        CandidateConfidence::High
    } else if score >= 70 {
        CandidateConfidence::Medium
    } else {
        CandidateConfidence::Low
    };

    Ok(Some(OutputCandidate {
        path: relative.to_path_buf(),
        kind,
        confidence,
        size_bytes: stamp.size_bytes,
        score,
    }))
}

fn generic_kind(relative: &Path, stamp: &FileStamp) -> Option<(String, i32)> {
    let lower = relative.to_string_lossy().to_ascii_lowercase();
    let extension = relative
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if matches!(extension.as_str(), "rpm" | "apk" | "gem" | "nupkg" | "crate") {
        return Some(("package archive".to_string(), 88));
    }
    if matches!(extension.as_str(), "so" | "dylib" | "dll") || lower.contains(".so.") {
        return Some(("shared library".to_string(), 82));
    }
    if matches!(extension.as_str(), "a" | "lib") {
        return Some(("static library".to_string(), 78));
    }
    if matches!(extension.as_str(), "exe" | "bin") {
        return Some(("executable artifact".to_string(), 82));
    }

    #[cfg(unix)]
    if stamp.mode & 0o111 != 0 && !source_like_extension(&extension) {
        return Some(("executable file".to_string(), 72));
    }

    None
}

fn source_like_extension(extension: &str) -> bool {
    matches!(
        extension,
        "c"
            | "cc"
            | "cpp"
            | "cxx"
            | "h"
            | "hpp"
            | "rs"
            | "go"
            | "java"
            | "kt"
            | "py"
            | "js"
            | "mjs"
            | "cjs"
            | "ts"
            | "tsx"
            | "jsx"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "pl"
            | "rb"
    )
}

fn obvious_intermediate_extension(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "o" | "obj" | "d" | "dep" | "rmeta" | "class" | "pyc" | "pyo" | "tmp" | "temp"
            | "log" | "stamp"
    )
}

fn likely_final_directory(path: &Path) -> bool {
    let value = path.to_string_lossy().replace('\\', "/");
    value.starts_with("target/release/")
        || value.starts_with("dist/")
        || value.starts_with("bin/")
        || value.starts_with("out/")
        || value.starts_with("bazel-bin/")
        || value.starts_with("build/bin/")
        || value.starts_with("build/libs/")
}

fn likely_intermediate_path(path: &Path) -> bool {
    let value = path.to_string_lossy().replace('\\', "/");
    value.contains("/CMakeFiles/")
        || value.starts_with("CMakeFiles/")
        || value.contains("/incremental/")
        || value.contains("/build/")
        || value.contains("/deps/")
        || value.contains("/tmp/")
        || value.contains("/classes/")
        || value.contains("/generated/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_tail_capture_is_bounded() {
        let mut bytes = vec![b'A'; FAILURE_OUTPUT_BYTES + 1024];
        bytes.extend_from_slice(b"FINAL");
        let captured = drain_tail(std::io::Cursor::new(bytes)).unwrap();

        assert!(captured.len() <= FAILURE_OUTPUT_BYTES);
        assert!(captured.ends_with("FINAL"));
    }

    #[test]
    fn ranks_final_binary_and_filters_object_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("target/release/deps")).unwrap();
        let before = snapshot(dir.path()).unwrap();

        fs::write(dir.path().join("target/release/app"), b"\x7fELFdemo").unwrap();
        fs::write(dir.path().join("target/release/deps/app.o"), b"\x7fELFobject").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.path().join("target/release/app");
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).unwrap();
        }

        let after = snapshot(dir.path()).unwrap();
        let (mut candidates, changed) =
            candidates_from_snapshots(dir.path(), &before, &after).unwrap();
        candidates.sort_by(|left, right| right.score.cmp(&left.score));

        assert_eq!(changed, 2);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].path, PathBuf::from("target/release/app"));
        assert_eq!(candidates[0].confidence, CandidateConfidence::High);
    }

    #[test]
    fn recognizes_package_candidate() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("dist")).unwrap();
        let before = snapshot(dir.path()).unwrap();
        fs::write(dir.path().join("dist/demo.whl"), b"PK\x03\x04demo").unwrap();
        let after = snapshot(dir.path()).unwrap();

        let (candidates, changed) =
            candidates_from_snapshots(dir.path(), &before, &after).unwrap();

        assert_eq!(changed, 1);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].path, PathBuf::from("dist/demo.whl"));
        assert_eq!(candidates[0].kind, "Python wheel");
        assert_eq!(candidates[0].confidence, CandidateConfidence::High);
    }
}
