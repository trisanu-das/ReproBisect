//! Deterministic, local-only derived views. Canonical evidence is never rewritten.
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{compat, model, schema::EvidenceKind};

pub const POLICY_VERSION: &str = "p03-disclosure-v1";
pub const RENDERER_VERSION: &str = "p03-renderer-v1";
pub const MAX_INPUT_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_MEMBER_BYTES: usize = 128 * 1024;
pub const MAX_BUNDLE_BYTES: usize = 192 * 1024;
pub const MAX_PREVIEW_BYTES: usize = 1024 * 1024;
const MAX_RECORDS: usize = 128;
pub const MAX_FIELD_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum AllowedField {
    ControlledValues,
    ArtifactPaths,
    PackageMetadata,
}

#[derive(Debug, Serialize)]
pub struct ControlledValue {
    pub kind: model::InterventionKind,
    pub variable: String,
    pub baseline: String,
    pub variant: String,
}

#[derive(Debug, Serialize)]
pub struct SummaryView {
    pub schema_version: u32,
    pub policy_version: &'static str,
    pub renderer_version: &'static str,
    pub kind: EvidenceKind,
    pub original_input_sha256: String,
    pub source_schema_version: u32,
    pub normalized_schema_version: u32,
    pub diagnostic_status: Option<model::CheckStatus>,
    pub comparison_status: Option<model::EnvironmentComparisonStatus>,
    pub build_exit_code: Option<i32>,
    pub observed_failures: usize,
    pub fix_candidates: usize,
    pub reported_verified_repairs: usize,
    pub coverage: SummaryCoverage,
    pub completion: &'static str,
    pub execution_completed: Option<bool>,
    pub execution_reason: Option<&'static str>,
    pub observed_runs: usize,
    pub artifacts: Vec<ArtifactView>,
    pub interventions: Vec<InterventionView>,
    pub allowed_fields: Vec<AllowedField>,
    pub controlled_values: Vec<ControlledValue>,
    pub excluded: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct SummaryCoverage {
    pub status: &'static str,
    pub complete: Option<bool>,
    pub authoritative: Option<crate::ci::Coverage>,
}

#[derive(Debug, Serialize)]
pub struct ArtifactView {
    pub detected_type: model::ArtifactType,
    /// Validated digest supplied by the evidence, not newly hashed artifact bytes.
    pub reported_sha256: Option<String>,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_metadata: Option<PackageMetadata>,
}

/// Only these five standardized package-format fields can be explicitly selected.
#[derive(Debug, Serialize)]
pub struct PackageMetadata {
    pub wheel_version: Option<String>,
    pub metadata_version: Option<String>,
    pub debian_binary_version: Option<String>,
    pub image_layout_version: Option<String>,
    pub index_schema_version: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InterventionView {
    pub kind: model::InterventionKind,
    pub changed: bool,
    pub variant_stable: Option<bool>,
    pub reverted_to_baseline: Option<bool>,
    pub failed: bool,
}

pub fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Reject lexical escapes before resolving and inspect every existing ancestor without following links.
/// This is cooperative path safety, not isolation from concurrent same-account filesystem mutation.
fn checked_path(path: &Path, absent_leaf: bool) -> Result<PathBuf> {
    ensure!(
        !path.as_os_str().is_empty(),
        "disclosure path must not be empty"
    );
    ensure!(
        path.as_os_str().len() <= 4096,
        "disclosure path exceeds byte limit"
    );
    for component in path.components() {
        match component {
            Component::ParentDir => bail!("parent traversal is forbidden for disclosure paths"),
            Component::Normal(name) => safe_name(name)?,
            #[cfg(windows)]
            Component::Prefix(prefix) => ensure!(
                matches!(prefix.kind(), std::path::Prefix::Disk(_)),
                "unsupported disclosure path prefix"
            ),
            _ => {}
        }
    }
    #[cfg(windows)]
    ensure!(
        !path.has_root() || path.is_absolute(),
        "drive-relative disclosure path is forbidden"
    );
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        #[cfg(windows)]
        ensure!(
            !matches!(path.components().next(), Some(Component::Prefix(_))),
            "drive-relative disclosure path is forbidden"
        );
        std::env::current_dir()?.join(path)
    };
    let mut current = PathBuf::new();
    let components: Vec<_> = absolute.components().collect();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        if !matches!(component, Component::Normal(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                ensure!(
                    !is_link(&metadata),
                    "disclosure path contains a symlink or reparse point"
                );
                if index + 1 < components.len() {
                    ensure!(metadata.is_dir(), "disclosure ancestor must be a directory");
                }
                if !absent_leaf && index + 1 == components.len() {
                    ensure!(metadata.is_file(), "summary input must be a regular file");
                }
            }
            Err(error)
                if absent_leaf
                    && index + 1 == components.len()
                    && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => bail!("cannot inspect disclosure path"),
        }
    }
    Ok(absolute)
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn safe_name(name: &std::ffi::OsStr) -> Result<()> {
    let name = name.to_str().context("disclosure path must be UTF-8")?;
    ensure!(
        name.len() <= 255 && !name.ends_with(['.', ' ']),
        "unsafe disclosure path component"
    );
    ensure!(
        !name
            .chars()
            .any(|c| c.is_control() || matches!(c, ':' | '\\' | '<' | '>' | '"' | '|' | '?' | '*')),
        "unsafe disclosure path component"
    );
    let stem = name.split('.').next().unwrap_or("").to_uppercase();
    ensure!(
        !matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) && !["COM", "LPT"]
            .iter()
            .any(
                |prefix| stem.strip_prefix(prefix).is_some_and(|n| (n.len() == 1
                    && (b'1'..=b'9').contains(&n.as_bytes()[0]))
                    || matches!(n, "¹" | "²" | "³"))
            ),
        "Windows reserved disclosure name"
    );
    Ok(())
}

/// Only the captured bytes are hashed and parsed. No second read of the source.
/// compat's path-only API reads a private, write-once snapshot, not the mutable input.
pub fn load_summary(input: &Path, fields: &[AllowedField]) -> Result<SummaryView> {
    let input = checked_path(input, false)?;
    let file = fs::File::open(&input).context("cannot open summary input")?;
    ensure!(
        file.metadata()?.is_file(),
        "summary input must be a regular file"
    );
    let mut raw = Vec::new();
    file.take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut raw)
        .context("cannot capture summary input")?;
    ensure!(
        raw.len() as u64 <= MAX_INPUT_BYTES,
        "summary input exceeds byte limit"
    );
    let snapshot =
        tempfile::NamedTempFile::new().context("cannot create private input snapshot")?;
    snapshot.as_file().write_all(&raw)?;
    snapshot.as_file().flush()?;
    let loaded = compat::load_evidence(snapshot.path()).context("invalid summary evidence")?;
    let mut allowed_fields = fields.to_vec();
    allowed_fields.sort();
    allowed_fields.dedup();
    let mut view = SummaryView {
        schema_version: 1,
        policy_version: POLICY_VERSION,
        renderer_version: RENDERER_VERSION,
        kind: loaded.document.kind(),
        original_input_sha256: sha256(&raw),
        source_schema_version: loaded.summary.source_schema_version,
        normalized_schema_version: loaded.summary.normalized_schema_version,
        diagnostic_status: None,
        comparison_status: None,
        build_exit_code: None,
        observed_failures: 0,
        fix_candidates: 0,
        reported_verified_repairs: 0,
        coverage: SummaryCoverage {
            status: "unknown",
            complete: None,
            authoritative: None,
        },
        completion: "unknown",
        observed_runs: 0,
        execution_completed: None,
        execution_reason: None,
        artifacts: Vec::new(),
        interventions: Vec::new(),
        allowed_fields,
        controlled_values: Vec::new(),
        excluded: vec![
            "raw_logs",
            "source_and_binaries",
            "ordinary_environment",
            "private_paths",
            "controlled_values",
            "package_metadata",
            "free_form_diagnostics",
        ],
    };
    match &loaded.document {
        compat::EvidenceDocument::CheckReport(report) => view.add_check(report)?,
        compat::EvidenceDocument::BuildRun(run) => {
            view.build_exit_code = Some(run.exit_code);
            view.add_run(run)?;
        }
        compat::EvidenceDocument::BuildFailure(failure) => {
            view.build_exit_code = Some(failure.exit_code);
            view.observed_failures = 1;
        }
        compat::EvidenceDocument::FixReport(report) => {
            view.add_check(&report.diagnosis)?;
            view.add_completion(&report.notes);
            ensure!(
                report.candidates.len() <= MAX_RECORDS && report.verifications.len() <= MAX_RECORDS,
                "summary record limit exceeded"
            );
            view.fix_candidates = report.candidates.len();
            view.reported_verified_repairs =
                report.verifications.iter().filter(|v| v.verified).count();
            for verification in &report.verifications {
                for run in verification
                    .baseline_runs
                    .iter()
                    .chain(&verification.intervention_runs)
                {
                    view.add_run(run)?;
                }
            }
        }
        compat::EvidenceDocument::EnvironmentComparisonReport(report) => {
            view.add_completion(&report.notes);
            view.comparison_status = Some(report.status.clone());
            for run in report
                .good_runs
                .iter()
                .chain(&report.bad_runs)
                .chain(&report.reproduction_runs)
                .chain(report.confirmation_run.iter())
            {
                view.add_run(run)?;
            }
            view.observed_failures = report.good_failures.len()
                + report.bad_failures.len()
                + report.reproduction_failures.len()
                + usize::from(report.confirmation_failure.is_some());
        }
    }
    ensure!(
        view.observed_failures <= MAX_RECORDS,
        "summary record limit exceeded"
    );
    view.excluded.retain(|field| match *field {
        "controlled_values" => !view
            .allowed_fields
            .contains(&AllowedField::ControlledValues),
        "package_metadata" => !view.allowed_fields.contains(&AllowedField::PackageMetadata),
        _ => true,
    });
    if !view.allowed_fields.contains(&AllowedField::ArtifactPaths) {
        view.excluded.push("artifact_paths");
    }
    Ok(view)
}

impl SummaryView {
    fn add_completion(&mut self, notes: &[String]) {
        if let Some(reason) = crate::engine::budget::completion_reason(notes) {
            self.completion = "incomplete";
            self.execution_completed = Some(false);
            self.execution_reason = Some(reason);
            self.coverage.status = "incomplete";
            self.coverage.complete = Some(false);
        }
    }
    fn add_run(&mut self, run: &model::BuildRun) -> Result<()> {
        ensure!(
            self.observed_runs < MAX_RECORDS,
            "summary record limit exceeded"
        );
        self.observed_runs += 1;
        for artifact in &run.artifacts {
            ensure!(
                self.artifacts.len() < MAX_RECORDS,
                "summary record limit exceeded"
            );
            let logical_path = if self.allowed_fields.contains(&AllowedField::ArtifactPaths) {
                Some(selected_string(
                    artifact
                        .logical_path
                        .to_str()
                        .context("selected artifact path must be UTF-8")?,
                )?)
            } else {
                None
            };
            let package_metadata = if self.allowed_fields.contains(&AllowedField::PackageMetadata) {
                artifact
                    .semantic_metadata
                    .as_ref()
                    .map(|metadata| {
                        let field = |key: &str| {
                            metadata
                                .attributes
                                .get(key)
                                .map(|v| selected_string(v))
                                .transpose()
                        };
                        Ok::<_, anyhow::Error>(PackageMetadata {
                            wheel_version: field("wheel_version")?,
                            metadata_version: field("metadata_version")?,
                            debian_binary_version: field("debian_binary_version")?,
                            image_layout_version: field("image_layout_version")?,
                            index_schema_version: field("index_schema_version")?,
                        })
                    })
                    .transpose()?
            } else {
                None
            };
            self.artifacts.push(ArtifactView {
                detected_type: artifact.detected_type.clone(),
                reported_sha256: valid_digest(&artifact.sha256),
                size_bytes: artifact.size_bytes,
                logical_path,
                package_metadata,
            });
        }
        Ok(())
    }
    fn add_check(&mut self, report: &model::CheckReport) -> Result<()> {
        self.add_completion(&report.notes);
        self.diagnostic_status = Some(report.status.clone());
        ensure!(
            report.interventions.len() <= MAX_RECORDS,
            "summary record limit exceeded"
        );
        for run in &report.runs {
            self.add_run(run)?;
        }
        for result in &report.interventions {
            self.interventions.push(InterventionView {
                kind: result.intervention.kind,
                changed: result.changed,
                variant_stable: result.variant_stable,
                reverted_to_baseline: result.reverted_to_baseline,
                failed: result.error.is_some() || !result.build_failures.is_empty(),
            });
            if self
                .allowed_fields
                .contains(&AllowedField::ControlledValues)
            {
                let i = &result.intervention;
                self.controlled_values.push(ControlledValue {
                    kind: i.kind,
                    variable: selected_string(&i.variable)?,
                    baseline: selected_string(&i.baseline_value)?,
                    variant: selected_string(&i.variant_value)?,
                });
            }
            for run in result
                .runs
                .iter()
                .chain(&result.reference_runs)
                .chain(result.confirmation_run.iter())
            {
                self.add_run(run)?;
            }
            self.observed_failures += result.build_failures.len();
        }
        if let Some(search) = &report.interaction_search {
            for run in search.runs.iter().chain(search.confirmation_run.iter()) {
                self.add_run(run)?;
            }
        }
        Ok(())
    }
}

fn selected_string(value: &str) -> Result<String> {
    ensure!(
        value.len() <= MAX_FIELD_BYTES,
        "selected field exceeds byte limit"
    );
    Ok(value.to_owned())
}

fn valid_digest(value: &str) -> Option<String> {
    (value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())).then(|| value.to_owned())
}

pub fn json_bytes(view: &SummaryView) -> Result<Vec<u8>> {
    let mut bytes = safe_json(view)?;
    bytes.push(b'\n');
    ensure!(
        bytes.len() <= MAX_MEMBER_BYTES,
        "summary output exceeds byte limit"
    );
    Ok(bytes)
}

/// JSON escapes retain exact values while neutralizing HTML, substring-recognized CI
/// commands, and Unicode controls in logs.
fn safe_json(value: &impl Serialize) -> Result<Vec<u8>> {
    let encoded = serde_json::to_string_pretty(value)?;
    let mut output = String::new();
    for c in encoded.chars() {
        if matches!(c, '<' | '>' | '&' | '#') || display_control(c) {
            use std::fmt::Write;
            write!(output, "\\u{:04x}", c as u32)?;
        } else {
            output.push(c);
        }
    }
    Ok(output.into_bytes())
}

fn display_control(c: char) -> bool {
    matches!(c, '\u{007f}'..='\u{009f}' | '\u{2028}' | '\u{2029}' | '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

pub fn markdown_bytes(view: &SummaryView) -> Result<Vec<u8>> {
    // Use ordinary JSON encoding here: the HTML renderer escapes the actual <>& bytes.
    let json = serde_json::to_string_pretty(view)?;
    let mut output = String::from(
        "# ReproBisect derived summary\n\nNot canonical evidence. Planned coverage is unknown unless supplied by the current CI calculation.\n\n<pre>\n",
    );
    for c in json.chars() {
        match c {
            c if display_control(c) => {
                use std::fmt::Write;
                write!(output, "&#92;u{:04x}", c as u32)?;
            }
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            ':' | '#' | '[' | ']' | '(' | ')' | '`' | '!' | '*' | '_' | '%' | '\\' => {
                use std::fmt::Write;
                write!(output, "&#{};", c as u32)?;
            }
            _ => output.push(c),
        }
    }
    output.push_str("\n</pre>\n");
    ensure!(
        output.len() <= MAX_MEMBER_BYTES,
        "summary output exceeds byte limit"
    );
    Ok(output.into_bytes())
}

/// CI-only authoritative coverage is copied from the existing calculation, never reconstructed.
/// Standalone evidence has no access to this trusted, current-run context.
pub fn write_sidecar(input: &Path, output: &Path, envelope: &crate::ci::CiEnvelope) -> Result<()> {
    let mut view = load_summary(input, &[])?;
    let reference = envelope
        .report
        .as_ref()
        .context("summary requires a persisted report")?;
    ensure!(
        view.original_input_sha256 == reference.sha256,
        "summary input differs from authoritative CI report"
    );
    let coverage = envelope
        .coverage
        .as_ref()
        .context("summary requires authoritative coverage")?;
    view.coverage.status = if coverage.complete {
        "complete"
    } else {
        "incomplete"
    };
    view.coverage.complete = Some(coverage.complete);
    view.coverage.authoritative = Some(coverage.clone());
    view.completion = if envelope.completion.completed {
        "completed"
    } else {
        "incomplete"
    };
    view.execution_completed = Some(envelope.completion.completed);
    view.execution_reason = view.execution_reason.or(Some(envelope.completion.reason));
    let bytes = json_bytes(&view)?;
    let output = checked_path(output, true)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .context("summary sidecar must be create-only")?;
    file.write_all(&bytes)
        .context("cannot write summary sidecar")?;
    file.flush().context("cannot flush summary sidecar")
}

#[derive(Debug, Serialize)]
struct Member {
    name: &'static str,
    size_bytes: usize,
    sha256: String,
    bytes: String,
}

impl Member {
    fn new(name: &'static str, bytes: Vec<u8>) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_MEMBER_BYTES,
            "export member exceeds byte limit"
        );
        Ok(Self {
            name,
            size_bytes: bytes.len(),
            sha256: sha256(&bytes),
            bytes: String::from_utf8(bytes)?,
        })
    }
    fn descriptor(&self) -> MemberDescriptor<'_> {
        MemberDescriptor {
            name: self.name,
            size_bytes: self.size_bytes,
            sha256: &self.sha256,
        }
    }
}

#[derive(Serialize)]
struct MemberDescriptor<'a> {
    name: &'static str,
    size_bytes: usize,
    sha256: &'a str,
}
#[derive(Serialize)]
struct Manifest<'a> {
    policy_version: &'static str,
    renderer_version: &'static str,
    original_input_sha256: &'a str,
    allowed_fields: &'a [AllowedField],
    members: Vec<MemberDescriptor<'a>>,
}
#[derive(Serialize)]
struct Approval<'a> {
    manifest: Manifest<'a>,
    destination: &'a str,
}
#[derive(Serialize)]
struct Preview<'a> {
    policy_version: &'static str,
    renderer_version: &'static str,
    original_input_sha256: &'a str,
    allowed_fields: &'a [AllowedField],
    destination: &'a str,
    approval_sha256: String,
    members: &'a [Member],
}

pub fn export(
    input: &Path,
    output: &Path,
    fields: &[AllowedField],
    approve: Option<&str>,
) -> Result<()> {
    let view = load_summary(input, fields)?;
    let checked = checked_path(output, true)?;
    let destination = fs::canonicalize(
        checked
            .parent()
            .context("export directory needs a parent")?,
    )?
    .join(
        checked
            .file_name()
            .context("export directory needs a name")?,
    );
    let destination_text = destination
        .to_str()
        .context("export directory must be UTF-8")?;
    let mut members = vec![
        Member::new("summary.json", json_bytes(&view)?)?,
        Member::new("summary.md", markdown_bytes(&view)?)?,
    ];
    let manifest = Manifest {
        policy_version: POLICY_VERSION,
        renderer_version: RENDERER_VERSION,
        original_input_sha256: &view.original_input_sha256,
        allowed_fields: &view.allowed_fields,
        members: members.iter().map(Member::descriptor).collect(),
    };
    let mut manifest_bytes = safe_json(&manifest)?;
    manifest_bytes.push(b'\n');
    members.push(Member::new("manifest.json", manifest_bytes)?);
    ensure!(
        members.iter().map(|m| m.size_bytes).sum::<usize>() <= MAX_BUNDLE_BYTES,
        "export aggregate byte limit exceeded"
    );
    let approval = Approval {
        manifest: Manifest {
            policy_version: POLICY_VERSION,
            renderer_version: RENDERER_VERSION,
            original_input_sha256: &view.original_input_sha256,
            allowed_fields: &view.allowed_fields,
            members: members.iter().map(Member::descriptor).collect(),
        },
        destination: destination_text,
    };
    let approval_sha256 = sha256(&serde_json::to_vec(&approval)?);
    let preview = Preview {
        policy_version: POLICY_VERSION,
        renderer_version: RENDERER_VERSION,
        original_input_sha256: &view.original_input_sha256,
        allowed_fields: &view.allowed_fields,
        destination: destination_text,
        approval_sha256,
        members: &members,
    };
    let mut preview_bytes = safe_json(&preview)?;
    preview_bytes.push(b'\n');
    ensure!(
        preview_bytes.len() <= MAX_PREVIEW_BYTES,
        "export preview byte limit exceeded"
    );
    if let Some(approve) = approve {
        ensure!(
            approve == preview.approval_sha256,
            "export approval does not match exact payload and destination"
        );
        fs::create_dir(&destination).context("export destination must be new")?;
        for member in &members {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination.join(member.name))?;
            file.write_all(member.bytes.as_bytes())?;
            file.flush()?;
        }
    }
    print_bytes(&preview_bytes)
}

/// Explicitly check both write and flush; transports must return exit 5, not panic.
pub fn print_bytes(bytes: &[u8]) -> Result<()> {
    #[cfg(unix)]
    use std::os::fd::AsFd;
    #[cfg(windows)]
    use std::os::windows::io::AsHandle;

    let stdout = std::io::stdout();
    let lock = stdout.lock();
    // Stdout can suppress errors on a closed descriptor. Keep its lock, but
    // duplicate the actual handle so both acquisition and writes fail closed.
    #[cfg(unix)]
    let owned = lock.as_fd().try_clone_to_owned();
    #[cfg(windows)]
    let owned = lock.as_handle().try_clone_to_owned();
    let mut output = fs::File::from(owned.context("cannot acquire derived stdout")?);
    output
        .write_all(bytes)
        .context("cannot write derived output")?;
    output.flush().context("cannot flush derived output")
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_path_requires_regular_file_before_open() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            super::checked_path(root.path(), false).is_err(),
            "nonregular source must be rejected by inspection, before potentially blocking open"
        );
        let regular = root.path().join("regular.json");
        std::fs::write(&regular, b"{}").unwrap();
        assert!(super::checked_path(&regular, false).is_ok());
    }
}
