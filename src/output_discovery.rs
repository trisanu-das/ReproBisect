use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};

use walkdir::{DirEntry, WalkDir};

use crate::{
    artifact::detect_type,
    model::{ArtifactType, RunnerBackend, SourceCopyOrder},
    source::copy_source_tree_with_context,
};

const MAX_SCAN_FILES: usize = 100_000;
const MAX_CANDIDATES: usize = 12;
const FAILURE_OUTPUT_BYTES: usize = 16 * 1024;
const BUILD_TIMEOUT_SECONDS: u64 = 600;

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
    let context =
        crate::engine::budget::ExecutionContext::new(&crate::config::ExecutionConfig::default())?;
    let result = discover_with_context(project, image, command, runner, &context);
    // Only an actually executed, closed-stream build-command failure is advisory.
    // Infrastructure errors that finish classifies later must still fail closed.
    let advisory = context.reason().is_none()
        && result
            .as_ref()
            .err()
            .is_some_and(|error| error.is::<DiscoveryBuildFailed>());
    let result = context.finish(project, result);
    // A receipt write error may replace even an ordinary executed build failure.
    // Only the original typed failure, not arbitrary finish errors, is advisory.
    if advisory
        && result
            .as_ref()
            .err()
            .is_some_and(|error| error.is::<DiscoveryBuildFailed>())
    {
        result
    } else {
        result.map_err(|error| anyhow::Error::new(DiscoveryInterrupted(error)))
    }
}
#[derive(Debug)]
pub struct DiscoveryInterrupted(anyhow::Error);
impl std::fmt::Display for DiscoveryInterrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "output discovery interrupted: {:#}", self.0)
    }
}
impl std::error::Error for DiscoveryInterrupted {}

/// An unsuccessful executed command is not an infrastructure interruption.
/// Constructed only after wait, cleanup and both captures have completed.
#[derive(Debug)]
struct DiscoveryBuildFailed {
    status: std::process::ExitStatus,
    tails: String,
}
impl std::fmt::Display for DiscoveryBuildFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "temporary discovery build failed with status {}{}",
            self.status, self.tails
        )
    }
}
impl std::error::Error for DiscoveryBuildFailed {}

fn discover_with_context(
    project: &Path,
    image: &str,
    command: &[String],
    runner: RunnerBackend,
    context: &crate::engine::budget::ExecutionContext,
) -> Result<OutputDiscoveryResult> {
    context.check()?;
    if command.is_empty() {
        bail!("cannot probe outputs with an empty build command");
    }

    let workspace = tempfile::Builder::new()
        .prefix("reprobisect-output-discovery-")
        .tempdir()
        .context("cannot create temporary output-discovery workspace")?;
    copy_source_tree_with_context(project, workspace.path(), SourceCopyOrder::Sorted, context)
        .context("cannot prepare temporary output-discovery workspace")?;

    let before = snapshot_checked(workspace.path(), &|| context.check())?;
    run_build(project, workspace.path(), image, command, runner, context)?;
    let after = snapshot_checked(workspace.path(), &|| context.check())?;

    let (mut candidates, changed_file_count) =
        candidates_from_snapshots(workspace.path(), &before, &after)?;
    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
    });
    candidates.truncate(MAX_CANDIDATES);

    context.check()?;
    Ok(OutputDiscoveryResult {
        candidates,
        changed_file_count,
        scan_truncated: before.truncated || after.truncated,
    })
}

fn run_build(
    project: &Path,
    workspace: &Path,
    image: &str,
    build_command: &[String],
    runner: RunnerBackend,
    context: &crate::engine::budget::ExecutionContext,
) -> Result<()> {
    context.dispatch_build()?;
    let runtime = runner.executable();
    let mut owned = crate::engine::budget::OwnedContainer::new(context, runner, "discovery")?;
    let container_name = owned.name();
    let mut command = Command::new(runtime);
    command
        .arg("run")
        .arg("--cidfile")
        .arg(owned.cidfile())
        .arg("--name")
        .arg(container_name)
        .arg("--label")
        .arg(context.operation_label())
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

    context.check()?;
    let mut child = command.spawn().with_context(|| {
        format!(
            "cannot execute {} for output discovery; run reprobisect doctor to check runtime readiness",
            runner.display_name()
        )
    })?;
    owned.launched();
    let stdout = child
        .stdout
        .take()
        .context("output-discovery stdout pipe was not available")?;
    let stderr = child
        .stderr
        .take()
        .context("output-discovery stderr pipe was not available")?;
    let stdout_live =
        crate::engine::budget::LiveCapture::start(stdout, FAILURE_OUTPUT_BYTES as u64);
    let stderr_live =
        crate::engine::budget::LiveCapture::start(stderr, FAILURE_OUTPUT_BYTES as u64);
    let status_result = context.wait_child(&mut child, Duration::from_secs(BUILD_TIMEOUT_SECONDS));
    let cleanup_result = owned.cleanup();
    let stdout = stdout_live.snapshot();
    let stderr = stderr_live.snapshot();
    let completion = context.check();
    if status_result.is_err()
        || cleanup_result.is_err()
        || completion.is_err()
        || !stdout.complete
        || !stderr.complete
    {
        context.stop("operational_error");
        context.persist_partial_attempt(project, serde_json::json!({
            "purpose": "discovery", "stdout": stdout, "stderr": stderr,
            "exit_status": status_result.as_ref().ok().and_then(|s| s.code()),
            "cleanup_verified": cleanup_result.is_ok(),
            "artifacts": crate::engine::budget::interrupted_artifacts(workspace, &[PathBuf::from(".")]),
            "qualification": "interrupted_discovery_not_completed_output_inference",
        }))?;
        status_result?;
        cleanup_result?;
        completion?;
        bail!("discovery output pipes did not close completely");
    }
    let status = status_result?;
    let stdout = stdout.text;
    let stderr = stderr.text;

    if !status.success() {
        return Err(anyhow::Error::new(DiscoveryBuildFailed {
            status,
            tails: failure_tails(&stdout, &stderr),
        }));
    }

    Ok(())
}

fn failure_tails(stdout: &str, stderr: &str) -> String {
    let mut message = String::new();
    if !stderr.is_empty() {
        message.push_str("\nstderr (tail):\n");
        message.push_str(stderr);
    }
    if !stdout.is_empty() {
        message.push_str("\nstdout (tail):\n");
        message.push_str(stdout);
    }
    message
}

#[cfg(test)]
fn snapshot(root: &Path) -> Result<Snapshot> {
    snapshot_checked(root, &|| Ok(()))
}
fn snapshot_checked(root: &Path, check: &impl Fn() -> Result<()>) -> Result<Snapshot> {
    check()?;
    let mut files = BTreeMap::new();
    let mut truncated = false;

    let walker = WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(should_descend);

    for entry in walker {
        check()?;
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

    if matches!(
        extension.as_str(),
        "rpm" | "apk" | "gem" | "nupkg" | "crate"
    ) {
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
        "c" | "cc"
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
        "o" | "obj"
            | "d"
            | "dep"
            | "rmeta"
            | "class"
            | "pyc"
            | "pyo"
            | "tmp"
            | "temp"
            | "log"
            | "stamp"
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
    fn discovery_entry_cannot_bypass_operation_deadline() {
        let project = tempfile::tempdir().unwrap();
        let context =
            crate::engine::budget::ExecutionContext::new(&crate::config::ExecutionConfig {
                total_timeout_seconds: 0,
                ..Default::default()
            })
            .unwrap();
        assert!(
            run_build(
                project.path(),
                project.path(),
                "unused",
                &["true".into()],
                RunnerBackend::Docker,
                &context
            )
            .is_err()
        );
        assert_eq!(context.reason(), Some("deadline_exhausted"));
    }

    #[test]
    fn live_discovery_capture_is_bounded() {
        let mut bytes = vec![b'A'; FAILURE_OUTPUT_BYTES + 1024];
        bytes.extend_from_slice(b"FINAL");
        let captured = crate::engine::budget::LiveCapture::start(
            std::io::Cursor::new(bytes),
            FAILURE_OUTPUT_BYTES as u64,
        )
        .snapshot();
        assert!(captured.text.len() <= FAILURE_OUTPUT_BYTES);
        assert!(captured.complete);
        assert!(captured.retained_truncated);
        assert_eq!(
            captured.observed_bytes,
            (FAILURE_OUTPUT_BYTES + 1029) as u64
        );
    }

    #[test]
    fn ranks_final_binary_and_filters_object_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("target/release/deps")).unwrap();
        let before = snapshot(dir.path()).unwrap();

        fs::write(dir.path().join("target/release/app"), b"\x7fELFdemo").unwrap();
        fs::write(
            dir.path().join("target/release/deps/app.o"),
            b"\x7fELFobject",
        )
        .unwrap();

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

        let (candidates, changed) = candidates_from_snapshots(dir.path(), &before, &after).unwrap();

        assert_eq!(changed, 1);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].path, PathBuf::from("dist/demo.whl"));
        assert_eq!(candidates[0].kind, "Python wheel");
        assert_eq!(candidates[0].confidence, CandidateConfidence::High);
    }
}
