use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    config::Config,
    evidence::persist_run,
    model::{
        ArchiveMetadataField, BuildRun, BuildSpec, CheckReport, CheckStatus, Confidence,
        ControlledEnvironment, InterventionKind, InterventionResult, SourceCopyOrder,
    },
    runner::{Runner, configured_runner},
    schema::FIX_REPORT_SCHEMA_CURRENT,
    source::{copy_source_tree, digest_tree, sha256_file},
};

use super::{
    control::{CheckOptions, check_project},
    interventions::{baseline_environment, plan_interventions},
};

const VERIFICATION_RUNS: usize = 2;

#[derive(Debug, Clone)]
pub struct FixOptions {
    pub check: CheckOptions,
    pub verify: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectPatchOperation {
    Append { text: String },
    InsertAfterLine { line_number: usize, text: String },
    Create { text: String },
    /// Replace exactly one expected literal site in an existing text file.
    ReplaceText { expected: String, replacement: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPatch {
    pub path: PathBuf,
    /// SHA-256 precondition for modifications of an existing file. Creation
    /// patches use None and refuse to overwrite any existing path.
    #[serde(default)]
    pub expected_sha256: Option<String>,
    pub operation: ProjectPatchOperation,
    pub unified_diff: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixPlan {
    pub id: String,
    pub title: String,
    pub causal_variable: String,
    pub strategy: String,
    #[serde(default)]
    pub suggested_environment_appends: BTreeMap<String, String>,
    #[serde(default)]
    pub suggested_environment_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub suggested_runner_controls: BTreeMap<String, String>,
    #[serde(default)]
    pub suggested_shell_commands: Vec<String>,
    #[serde(default)]
    pub project_patch: Option<ProjectPatch>,
    pub automatic_verification_supported: bool,
    pub evidence: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixVerification {
    pub plan_id: String,
    pub experiment_id: Option<Uuid>,
    pub attempted: bool,
    pub verified: bool,
    pub baseline_runs: Vec<BuildRun>,
    pub intervention_runs: Vec<BuildRun>,
    pub evidence: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixReport {
    pub schema_version: u32,
    pub diagnosis: CheckReport,
    pub candidates: Vec<FixPlan>,
    pub verifications: Vec<FixVerification>,
    pub notes: Vec<String>,
}

pub fn fix_project(
    project_root: &Path,
    config: &Config,
    options: &FixOptions,
) -> Result<FixReport> {
    let diagnosis = check_project(project_root, config, &options.check)?;
    let candidates = generate_fix_plans(project_root, config, &diagnosis);

    let mut verifications = Vec::new();
    if options.verify {
        for plan in &candidates {
            verifications.push(verify_plan(project_root, config, &diagnosis, plan));
        }
    }

    let mut notes = vec![
        "fix candidates are rule-based; project patches are applied only to temporary verification copies and never to the original source tree automatically"
            .to_string(),
    ];
    if candidates.is_empty() && diagnosis.status == CheckStatus::NonReproducible {
        notes.push(
            "no supported automatic fix rule matched the evidence; use the diagnosis remediation as manual guidance"
                .to_string(),
        );
    }
    if options.verify {
        notes.push(
            "a fix is marked verified only when repeated fixed-baseline builds are stable and the original failing intervention no longer changes artifact bytes"
                .to_string(),
        );
    }

    let report = FixReport {
        schema_version: FIX_REPORT_SCHEMA_CURRENT,
        diagnosis,
        candidates,
        verifications,
        notes,
    };
    persist_fix_report(project_root, &report)?;
    Ok(report)
}

fn generate_fix_plans(project_root: &Path, config: &Config, report: &CheckReport) -> Vec<FixPlan> {
    let mut plans = Vec::new();
    for result in &report.interventions {
        if !result.changed || !supported_confidence(report, result) {
            continue;
        }

        match result.intervention.kind {
            InterventionKind::SourcePath | InterventionKind::BuildPath
                if has_path_elf_pair(result) =>
            {
                plans.push(path_remap_plan(project_root, config, result));
            }
            InterventionKind::SourceDateEpoch => {
                plans.push(source_date_epoch_plan(project_root, config, result));
            }
            InterventionKind::SourceMtime if has_archive_field(result, ArchiveMetadataField::Mtime)
                || has_archive_field(result, ArchiveMetadataField::ContainerMtime) =>
            {
                plans.push(source_mtime_plan(project_root, config, result));
            }
            InterventionKind::Umask if has_archive_field(result, ArchiveMetadataField::Mode) => {
                plans.push(umask_plan(project_root, config, result));
            }
            _ => {}
        }
    }
    plans
}

fn supported_confidence(report: &CheckReport, result: &InterventionResult) -> bool {
    report.diagnoses.iter().any(|diagnosis| {
        diagnosis
            .causal_variables
            .iter()
            .any(|variable| variable == &result.intervention.variable)
            && matches!(diagnosis.confidence, Confidence::High | Confidence::Medium)
    })
}

fn has_archive_field(result: &InterventionResult, field: ArchiveMetadataField) -> bool {
    result.artifact_deltas.iter().any(|delta| {
        delta.changed
            && delta
                .archive_metadata_evidence
                .iter()
                .any(|difference| difference.field == field)
    })
}

fn has_path_elf_pair(result: &crate::model::InterventionResult) -> bool {
    result.artifact_deltas.iter().any(|delta| {
        if !delta.changed {
            return false;
        }
        let has_baseline = delta.elf_marker_location_evidence.iter().any(|location| {
            location.variable == result.intervention.variable
                && location.value == result.intervention.baseline_value
        });
        let has_variant = delta.elf_marker_location_evidence.iter().any(|location| {
            location.variable == result.intervention.variable
                && location.value == result.intervention.variant_value
        });
        has_baseline && has_variant
    })
}

fn path_remap_plan(
    project_root: &Path,
    config: &Config,
    result: &crate::model::InterventionResult,
) -> FixPlan {
    let baseline = &result.intervention.baseline_value;
    let variant = &result.intervention.variant_value;
    let gcc_append = format!(
        "-ffile-prefix-map={baseline}=. -fdebug-prefix-map={baseline}=. -ffile-prefix-map={variant}=. -fdebug-prefix-map={variant}=."
    );
    let rust_append = format!(
        "--remap-path-prefix={baseline}=. --remap-path-prefix={variant}=."
    );

    let mut suggested = BTreeMap::new();
    suggested.insert("CFLAGS".to_string(), gcc_append.clone());
    suggested.insert("CXXFLAGS".to_string(), gcc_append.clone());
    suggested.insert("RUSTFLAGS".to_string(), rust_append.clone());

    let path_kind = match result.intervention.kind {
        InterventionKind::SourcePath => "source",
        InterventionKind::BuildPath => "build",
        _ => unreachable!("path_remap_plan is only called for path interventions"),
    };
    let project_patch = path_project_patch(
        project_root,
        config,
        &format!("{}-prefix-map", result.intervention.id),
        path_kind,
        &gcc_append,
        &rust_append,
    );
    let automatic_verification_supported = project_patch.is_some()
        || suggested.keys().any(|key| config.build.env.contains_key(key));
    let mut evidence = Vec::new();
    for delta in result.artifact_deltas.iter().filter(|delta| delta.changed) {
        for location in &delta.elf_marker_location_evidence {
            evidence.push(format!(
                "{} contains {:?} in ELF section {}",
                delta.logical_path.display(),
                location.value,
                location.section
            ));
        }
    }
    evidence.sort();
    evidence.dedup();

    let mut limitations = vec![
        "the candidate targets GCC/Clang prefix-map flags and rustc path remapping; other toolchains require a toolchain-specific equivalent"
            .to_string(),
        if project_patch.is_some() {
            format!("a {} project patch is available and will be verified in a temporary project copy", project_patch.as_ref().map(|patch| patch.path.display().to_string()).unwrap_or_default())
        } else {
            "no safe supported project-level patch point was found; verification falls back to configured compiler-flag environment variables when available".to_string()
        },
    ];
    if !automatic_verification_supported {
        limitations.push(
            "automatic verification is unavailable because the build config does not already declare CFLAGS, CXXFLAGS, or RUSTFLAGS; injecting a new flag variable could change build semantics independently of the proposed fix"
                .to_string(),
        );
    }

    FixPlan {
        id: format!("{}-prefix-map", result.intervention.id),
        title: format!("Normalize embedded {path_kind} paths with compiler prefix mapping"),
        causal_variable: result.intervention.variable.clone(),
        strategy: "compiler-prefix-map".to_string(),
        suggested_environment_appends: suggested,
        suggested_environment_overrides: BTreeMap::new(),
        suggested_runner_controls: BTreeMap::new(),
        suggested_shell_commands: Vec::new(),
        project_patch,
        automatic_verification_supported,
        evidence,
        limitations,
    }
}

fn source_date_epoch_plan(project_root: &Path, config: &Config, result: &InterventionResult) -> FixPlan {
    let value = result.intervention.baseline_value.clone();
    let mut overrides = BTreeMap::new();
    overrides.insert("SOURCE_DATE_EPOCH".to_string(), value.clone());
    let project_patch = source_date_epoch_project_patch(
        project_root,
        config,
        &format!("{}-pin", result.intervention.id),
        &value,
    );
    let project_patch_path = project_patch
        .as_ref()
        .map(|patch| patch.path.display().to_string());

    FixPlan {
        id: format!("{}-pin", result.intervention.id),
        title: "Pin the standardized build timestamp input".to_string(),
        causal_variable: result.intervention.variable.clone(),
        strategy: "pin-source-date-epoch".to_string(),
        suggested_environment_appends: BTreeMap::new(),
        suggested_environment_overrides: overrides,
        suggested_runner_controls: BTreeMap::new(),
        suggested_shell_commands: vec![format!("export SOURCE_DATE_EPOCH={value}")],
        project_patch,
        automatic_verification_supported: true,
        evidence: result
            .artifact_deltas
            .iter()
            .filter(|delta| delta.changed)
            .map(|delta| {
                format!(
                    "{} changed when SOURCE_DATE_EPOCH changed from {} to {}",
                    delta.logical_path.display(),
                    result.intervention.baseline_value,
                    result.intervention.variant_value
                )
            })
            .collect(),
        limitations: vec![
            "this is an operational normalization candidate; if SOURCE_DATE_EPOCH is intentionally part of the declared build identity, pinning it changes that build input rather than removing timestamp semantics".to_string(),
            if let Some(path) = project_patch_path {
                format!("the project-level candidate pins SOURCE_DATE_EPOCH through {path}; it does not rewrite timestamp-generating source code")
            } else {
                "no safe supported project-level timestamp patch point was found, so verification uses runner-level normalization only".to_string()
            },
        ],
    }
}

fn source_mtime_plan(project_root: &Path, config: &Config, result: &InterventionResult) -> FixPlan {
    let value = result.intervention.baseline_value.clone();
    let mut controls = BTreeMap::new();
    controls.insert("source_mtime_epoch".to_string(), value.clone());
    let project_patch = archive_mtime_project_patch(
        project_root,
        config,
        &format!("{}-archive-mtime", result.intervention.id),
        &value,
    );

    let mut evidence = Vec::new();
    for delta in result.artifact_deltas.iter().filter(|delta| delta.changed) {
        for difference in &delta.archive_metadata_evidence {
            if matches!(difference.field, ArchiveMetadataField::Mtime | ArchiveMetadataField::ContainerMtime) {
                evidence.push(format!(
                    "{} archive {:?} changed for {:?}",
                    delta.logical_path.display(), difference.field, difference.member
                ));
            }
        }
    }
    evidence.sort();
    evidence.dedup();

    FixPlan {
        id: format!("{}-normalize", result.intervention.id),
        title: "Normalize source/archive mtimes before packaging".to_string(),
        causal_variable: result.intervention.variable.clone(),
        strategy: "normalize-source-mtime".to_string(),
        suggested_environment_appends: BTreeMap::new(),
        suggested_environment_overrides: BTreeMap::new(),
        suggested_runner_controls: controls,
        suggested_shell_commands: vec![format!(
            "find . \\( -type f -o -type d \\) -exec touch -d '@{value}' {{}} +"
        )],
        project_patch: project_patch.clone(),
        automatic_verification_supported: true,
        evidence,
        limitations: vec![
            if project_patch.is_some() {
                "the project-level candidate uses GNU tar's TAR_OPTIONS to normalize member mtimes; verification determines whether the project's actual tar invocation honors it".to_string()
            } else {
                "no conservative project-level GNU tar patch point was found; verification falls back to normalizing the copied source snapshot before the build".to_string()
            },
            "the generic shell suggestion assumes GNU-compatible find/touch; archive-native timestamp controls are preferable when available".to_string(),
        ],
    }
}

fn umask_plan(project_root: &Path, config: &Config, result: &InterventionResult) -> FixPlan {
    let value = result.intervention.baseline_value.clone();
    let mut controls = BTreeMap::new();
    controls.insert("umask".to_string(), value.clone());
    let project_patch = archive_mode_project_patch(
        project_root,
        config,
        &format!("{}-archive-mode", result.intervention.id),
    );

    let mut evidence = Vec::new();
    for delta in result.artifact_deltas.iter().filter(|delta| delta.changed) {
        for difference in &delta.archive_metadata_evidence {
            if difference.field == ArchiveMetadataField::Mode {
                evidence.push(format!(
                    "{} archive member {:?} mode changed from {:?} to {:?}",
                    delta.logical_path.display(),
                    difference.member,
                    difference.baseline,
                    difference.variant
                ));
            }
        }
    }
    evidence.sort();
    evidence.dedup();

    FixPlan {
        id: format!("{}-pin", result.intervention.id),
        title: "Pin the build umask / normalize output modes".to_string(),
        causal_variable: result.intervention.variable.clone(),
        strategy: "pin-umask".to_string(),
        suggested_environment_appends: BTreeMap::new(),
        suggested_environment_overrides: BTreeMap::new(),
        suggested_runner_controls: controls,
        suggested_shell_commands: vec![format!("umask {value}")],
        project_patch: project_patch.clone(),
        automatic_verification_supported: true,
        evidence,
        limitations: vec![
            if project_patch.is_some() {
                "the project-level candidate uses GNU tar's --mode transformation through TAR_OPTIONS; verification determines whether it covers the affected archive command".to_string()
            } else {
                "no conservative project-level GNU tar patch point was found; verification falls back to pinning the build umask operationally".to_string()
            },
            "explicitly setting archive member permissions is more portable than inheriting process umask; non-GNU tar implementations may require different flags".to_string(),
        ],
    }
}

fn archive_makefile_patch(
    project_root: &Path,
    config: &Config,
    id: &str,
    body: &str,
) -> Option<ProjectPatch> {
    if !build_command_hint(config).contains("make") {
        return None;
    }
    let makefile = project_root.join("Makefile");
    let metadata = fs::symlink_metadata(&makefile).ok()?;
    if !metadata.file_type().is_file() {
        return None;
    }
    let contents = fs::read_to_string(&makefile).ok()?;
    if !contents.to_ascii_lowercase().contains("tar ")
        && !contents.to_ascii_lowercase().contains("tar-")
        && !contents.to_ascii_lowercase().contains("$(tar)")
    {
        return None;
    }
    makefile_append_patch(project_root, id, body)
}

fn unique_literal_replacement(
    text: &str,
    candidates: &[(String, String)],
) -> Option<(String, String)> {
    let mut matches = Vec::new();
    for (expected, replacement) in candidates {
        for _ in text.match_indices(expected) {
            matches.push((expected.clone(), replacement.clone()));
        }
    }
    if matches.len() == 1 { matches.pop() } else { None }
}

fn cmake_archive_patch(
    project_root: &Path,
    id: &str,
    tar_options: &str,
) -> Option<ProjectPatch> {
    let relative = PathBuf::from("CMakeLists.txt");
    let text = fs::read_to_string(project_root.join(&relative)).ok()?;
    if text.contains(&format!("ReproBisect candidate: {id}")) {
        return None;
    }
    let env_prefix = format!(
        "COMMAND ${{CMAKE_COMMAND}} -E env \"TAR_OPTIONS={tar_options}\" "
    );
    let candidates = vec![
        ("COMMAND tar ".to_string(), format!("{env_prefix}tar ")),
        (
            "COMMAND /usr/bin/tar ".to_string(),
            format!("{env_prefix}/usr/bin/tar "),
        ),
    ];
    let (expected, replacement) = unique_literal_replacement(&text, &candidates)?;
    replace_text_patch(project_root, relative, &expected, &replacement)
}

fn meson_archive_patch(
    project_root: &Path,
    id: &str,
    tar_options: &str,
) -> Option<ProjectPatch> {
    let relative = PathBuf::from("meson.build");
    let text = fs::read_to_string(project_root.join(&relative)).ok()?;
    if text.contains(&format!("ReproBisect candidate: {id}")) {
        return None;
    }
    let candidates = vec![
        (
            "run_command('tar',".to_string(),
            format!("run_command('env', 'TAR_OPTIONS={tar_options}', 'tar',"),
        ),
        (
            "run_command(\"tar\",".to_string(),
            format!("run_command(\"env\", \"TAR_OPTIONS={tar_options}\", \"tar\","),
        ),
        (
            "command : ['tar',".to_string(),
            format!("command : ['env', 'TAR_OPTIONS={tar_options}', 'tar',"),
        ),
        (
            "command: ['tar',".to_string(),
            format!("command: ['env', 'TAR_OPTIONS={tar_options}', 'tar',"),
        ),
    ];
    let (expected, replacement) = unique_literal_replacement(&text, &candidates)?;
    replace_text_patch(project_root, relative, &expected, &replacement)
}

fn command_shell_scripts(config: &Config) -> Vec<PathBuf> {
    let mut scripts = Vec::new();
    for argument in &config.build.command {
        for token in argument.split_whitespace() {
            let token = token
                .trim_matches(|ch: char| matches!(ch, '\'' | '"' | ';' | '(' | ')'))
                .trim_start_matches("./");
            if !token.ends_with(".sh") {
                continue;
            }
            let path = PathBuf::from(token);
            if path.is_absolute()
                || path.components().any(|part| matches!(part, std::path::Component::ParentDir))
            {
                continue;
            }
            if !scripts.contains(&path) {
                scripts.push(path);
            }
        }
    }
    scripts
}

fn shell_tar_line_replacement(line: &str, tar_options: &str) -> Option<String> {
    let mut sites = Vec::new();
    for needle in ["/usr/bin/tar ", "tar "] {
        for (index, _) in line.match_indices(needle) {
            if needle == "tar " && index > 0 {
                let previous = line[..index].chars().next_back()?;
                if !previous.is_whitespace() && !matches!(previous, ';' | '&' | '|' | '(') {
                    continue;
                }
            }
            sites.push((index, needle));
        }
    }
    // `/usr/bin/tar ` also contains `tar `; remove the nested duplicate site.
    sites.sort_by_key(|(index, needle)| (*index, usize::MAX - needle.len()));
    sites.dedup_by(|left, right| {
        let left_end = left.0 + left.1.len();
        let right_end = right.0 + right.1.len();
        left.0 == right.0 || (left.0 <= right.0 && right_end <= left_end)
    });
    if sites.len() != 1 {
        return None;
    }
    let (index, needle) = sites[0];
    let command = &line[index..index + needle.len()];
    let replacement = format!("TAR_OPTIONS='{tar_options}' {command}");
    let mut updated = line.to_string();
    updated.replace_range(index..index + needle.len(), &replacement);
    Some(updated)
}

fn shell_archive_patch(
    project_root: &Path,
    config: &Config,
    tar_options: &str,
) -> Option<ProjectPatch> {
    let mut sites = Vec::new();
    for relative in command_shell_scripts(config) {
        let path = project_root.join(&relative);
        let metadata = fs::symlink_metadata(&path).ok()?;
        if !metadata.file_type().is_file() {
            continue;
        }
        let text = fs::read_to_string(&path).ok()?;
        for line in text.lines() {
            if line.trim_start().starts_with('#') {
                continue;
            }
            if let Some(replacement) = shell_tar_line_replacement(line, tar_options) {
                sites.push((relative.clone(), line.to_string(), replacement));
            }
        }
    }
    if sites.len() != 1 {
        return None;
    }
    let (relative, expected, replacement) = sites.pop()?;
    replace_text_patch(project_root, relative, &expected, &replacement)
}

fn archive_project_patch(
    project_root: &Path,
    config: &Config,
    id: &str,
    tar_options: &str,
    make_body: &str,
) -> Option<ProjectPatch> {
    let hint = build_command_hint(config);
    if hint.contains("make") {
        if let Some(patch) = archive_makefile_patch(project_root, config, id, make_body) {
            return Some(patch);
        }
    }
    if hint.contains("cmake") {
        if let Some(patch) = cmake_archive_patch(project_root, id, tar_options) {
            return Some(patch);
        }
    }
    if hint.contains("meson") {
        if let Some(patch) = meson_archive_patch(project_root, id, tar_options) {
            return Some(patch);
        }
    }
    shell_archive_patch(project_root, config, tar_options)
        .or_else(|| cmake_archive_patch(project_root, id, tar_options))
        .or_else(|| meson_archive_patch(project_root, id, tar_options))
        .or_else(|| archive_makefile_patch(project_root, config, id, make_body))
}

fn archive_mtime_project_patch(
    project_root: &Path,
    config: &Config,
    id: &str,
    epoch: &str,
) -> Option<ProjectPatch> {
    let tar_options = format!("--mtime=@{epoch} --clamp-mtime");
    let make_body = format!(
        "# ReproBisect candidate: {id}\n# Normalize GNU tar member timestamps.\nREPROBISECT_ARCHIVE_EPOCH ?= {epoch}\nTAR_OPTIONS := --mtime=@$(REPROBISECT_ARCHIVE_EPOCH) --clamp-mtime $(TAR_OPTIONS)\nexport TAR_OPTIONS\n"
    );
    archive_project_patch(project_root, config, id, &tar_options, &make_body)
}

fn archive_mode_project_patch(
    project_root: &Path,
    config: &Config,
    id: &str,
) -> Option<ProjectPatch> {
    let tar_options = "--mode=u+rwX,go+rX,go-w";
    let make_body = format!(
        "# ReproBisect candidate: {id}\n# Normalize GNU tar member permissions independent of ambient umask.\nTAR_OPTIONS := --mode=u+rwX,go+rX,go-w $(TAR_OPTIONS)\nexport TAR_OPTIONS\n"
    );
    archive_project_patch(project_root, config, id, tar_options, &make_body)
}

fn build_command_hint(config: &Config) -> String {
    config
        .build
        .command
        .join(" ")
        .to_ascii_lowercase()
}

fn append_existing_patch(
    project_root: &Path,
    relative: PathBuf,
    id: &str,
    block: &str,
) -> Option<ProjectPatch> {
    let path = project_root.join(&relative);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.file_type().is_file() {
        return None;
    }
    let original = fs::read_to_string(&path).ok()?;
    if original.contains(&format!("# ReproBisect candidate: {id}")) {
        return None;
    }
    let expected_sha256 = sha256_file(&path).ok()?;
    let text = if original.ends_with('\n') {
        format!("\n{block}")
    } else {
        format!("\n\n{block}")
    };
    let line_count = original.lines().count();
    let added = text
        .trim_start_matches('\n')
        .lines()
        .map(|line| format!("+{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let added_count = text.trim_start_matches('\n').lines().count();
    let unified_diff = format!(
        "--- a/{path}\n+++ b/{path}\n@@ -{line_count},0 +{},{} @@\n{}\n",
        line_count + 1,
        added_count,
        added,
        path = relative.display(),
    );
    Some(ProjectPatch {
        path: relative,
        expected_sha256: Some(expected_sha256),
        operation: ProjectPatchOperation::Append { text },
        unified_diff,
    })
}

fn insert_after_line_patch(
    project_root: &Path,
    relative: PathBuf,
    id: &str,
    line_number: usize,
    block: &str,
) -> Option<ProjectPatch> {
    let path = project_root.join(&relative);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.file_type().is_file() || line_number == 0 {
        return None;
    }
    let original = fs::read_to_string(&path).ok()?;
    if original.contains(&format!("# ReproBisect candidate: {id}"))
        || line_number > original.lines().count().max(1)
    {
        return None;
    }
    let expected_sha256 = sha256_file(&path).ok()?;
    let added = block
        .lines()
        .map(|line| format!("+{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let added_count = block.lines().count();
    let unified_diff = format!(
        "--- a/{path}\n+++ b/{path}\n@@ -{line_number},0 +{},{} @@\n{}\n",
        line_number + 1,
        added_count,
        added,
        path = relative.display(),
    );
    Some(ProjectPatch {
        path: relative,
        expected_sha256: Some(expected_sha256),
        operation: ProjectPatchOperation::InsertAfterLine {
            line_number,
            text: block.to_string(),
        },
        unified_diff,
    })
}

fn create_file_patch(project_root: &Path, relative: PathBuf, text: String) -> Option<ProjectPatch> {
    let path = project_root.join(&relative);
    if fs::symlink_metadata(&path).is_ok() {
        return None;
    }
    let added = text
        .lines()
        .map(|line| format!("+{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let line_count = text.lines().count();
    let unified_diff = format!(
        "--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{line_count} @@\n{added}\n",
        path = relative.display(),
    );
    Some(ProjectPatch {
        path: relative,
        expected_sha256: None,
        operation: ProjectPatchOperation::Create { text },
        unified_diff,
    })
}

fn replace_text_patch(
    project_root: &Path,
    relative: PathBuf,
    expected: &str,
    replacement: &str,
) -> Option<ProjectPatch> {
    let path = project_root.join(&relative);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.file_type().is_file() || expected.is_empty() || expected == replacement {
        return None;
    }
    let original = fs::read_to_string(&path).ok()?;
    if original.match_indices(expected).count() != 1 {
        return None;
    }
    let expected_sha256 = sha256_file(&path).ok()?;
    let index = original.find(expected)?;
    let line_number = original[..index].bytes().filter(|byte| *byte == b'\n').count() + 1;
    let old_lines = expected.lines().map(|line| format!("-{line}")).collect::<Vec<_>>().join("\n");
    let new_lines = replacement.lines().map(|line| format!("+{line}")).collect::<Vec<_>>().join("\n");
    let unified_diff = format!(
        "--- a/{path}\n+++ b/{path}\n@@ -{line_number},{} +{line_number},{} @@\n{old_lines}\n{new_lines}\n",
        expected.lines().count().max(1),
        replacement.lines().count().max(1),
        path = relative.display(),
    );
    Some(ProjectPatch {
        path: relative,
        expected_sha256: Some(expected_sha256),
        operation: ProjectPatchOperation::ReplaceText {
            expected: expected.to_string(),
            replacement: replacement.to_string(),
        },
        unified_diff,
    })
}

fn makefile_append_patch(project_root: &Path, id: &str, block: &str) -> Option<ProjectPatch> {
    append_existing_patch(project_root, PathBuf::from("Makefile"), id, block)
}

fn cmake_path_patch(
    project_root: &Path,
    id: &str,
    path_kind: &str,
    gcc_append: &str,
) -> Option<ProjectPatch> {
    append_existing_patch(
        project_root,
        PathBuf::from("CMakeLists.txt"),
        id,
        &format!(
            "# ReproBisect candidate: {id}\n# Normalize embedded {path_kind} paths for C/C++ targets\nset(CMAKE_C_FLAGS \"${{CMAKE_C_FLAGS}} {gcc_append}\")\nset(CMAKE_CXX_FLAGS \"${{CMAKE_CXX_FLAGS}} {gcc_append}\")\n"
        ),
    )
}

fn meson_project_end_line(text: &str) -> Option<usize> {
    let mut started = false;
    let mut depth = 0_i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (line_index, line) in text.lines().enumerate() {
        let chars = line.chars().collect::<Vec<_>>();
        let mut i = 0_usize;
        while i < chars.len() {
            let ch = chars[i];
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            if let Some(active) = quote {
                if ch == '\\' {
                    escaped = true;
                } else if ch == active {
                    quote = None;
                }
                i += 1;
                continue;
            }
            if ch == '#' {
                break;
            }
            if ch == '\'' || ch == '"' {
                quote = Some(ch);
                i += 1;
                continue;
            }
            if !started {
                let tail = chars[i..].iter().collect::<String>();
                if tail.starts_with("project(") {
                    started = true;
                    depth = 1;
                    i += "project(".len();
                    continue;
                }
            } else if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    return Some(line_index + 1);
                }
            }
            i += 1;
        }
    }
    None
}

fn meson_path_patch(
    project_root: &Path,
    id: &str,
    path_kind: &str,
    gcc_append: &str,
) -> Option<ProjectPatch> {
    let relative = PathBuf::from("meson.build");
    let original = fs::read_to_string(project_root.join(&relative)).ok()?;
    let line = meson_project_end_line(&original)?;
    let flags = gcc_append
        .split_whitespace()
        .map(|flag| format!("'{flag}'"))
        .collect::<Vec<_>>()
        .join(", ");
    insert_after_line_patch(
        project_root,
        relative,
        id,
        line,
        &format!(
            "# ReproBisect candidate: {id}\n# Normalize embedded {path_kind} paths before targets are declared\nadd_project_arguments({flags}, language : 'c')\nadd_project_arguments({flags}, language : 'cpp')\n"
        ),
    )
}

fn cargo_path_patch(
    project_root: &Path,
    id: &str,
    path_kind: &str,
    rust_append: &str,
) -> Option<ProjectPatch> {
    if !project_root.join("Cargo.toml").is_file()
        || project_root.join(".cargo/config").exists()
        || project_root.join(".cargo/config.toml").exists()
    {
        return None;
    }
    let flags = rust_append
        .split_whitespace()
        .map(|flag| format!("    {:?},", flag))
        .collect::<Vec<_>>()
        .join("\n");
    create_file_patch(
        project_root,
        PathBuf::from(".cargo/config.toml"),
        format!(
            "# ReproBisect candidate: {id}\n# Normalize embedded {path_kind} paths\n[build]\nrustflags = [\n{flags}\n]\n"
        ),
    )
}

fn path_project_patch(
    project_root: &Path,
    config: &Config,
    id: &str,
    path_kind: &str,
    gcc_append: &str,
    rust_append: &str,
) -> Option<ProjectPatch> {
    let hint = build_command_hint(config);
    if hint.contains("cargo") {
        if let Some(patch) = cargo_path_patch(project_root, id, path_kind, rust_append) {
            return Some(patch);
        }
    }
    if hint.contains("meson") {
        if let Some(patch) = meson_path_patch(project_root, id, path_kind, gcc_append) {
            return Some(patch);
        }
    }
    if hint.contains("cmake") {
        if let Some(patch) = cmake_path_patch(project_root, id, path_kind, gcc_append) {
            return Some(patch);
        }
    }
    if hint.contains("make") {
        if let Some(patch) = makefile_append_patch(
            project_root,
            id,
            &format!(
                "# ReproBisect candidate: {id}\n# Normalize embedded {path_kind} paths\nREPROBISECT_PREFIX_MAP_FLAGS := {gcc_append}\nREPROBISECT_RUST_PREFIX_MAP_FLAGS := {rust_append}\nCFLAGS += $(REPROBISECT_PREFIX_MAP_FLAGS)\nCXXFLAGS += $(REPROBISECT_PREFIX_MAP_FLAGS)\nRUSTFLAGS += $(REPROBISECT_RUST_PREFIX_MAP_FLAGS)\nexport CFLAGS CXXFLAGS RUSTFLAGS\n"
            ),
        ) {
            return Some(patch);
        }
    }
    makefile_append_patch(
        project_root,
        id,
        &format!(
            "# ReproBisect candidate: {id}\nREPROBISECT_PREFIX_MAP_FLAGS := {gcc_append}\nREPROBISECT_RUST_PREFIX_MAP_FLAGS := {rust_append}\nCFLAGS += $(REPROBISECT_PREFIX_MAP_FLAGS)\nCXXFLAGS += $(REPROBISECT_PREFIX_MAP_FLAGS)\nRUSTFLAGS += $(REPROBISECT_RUST_PREFIX_MAP_FLAGS)\nexport CFLAGS CXXFLAGS RUSTFLAGS\n"
        ),
    )
    .or_else(|| cmake_path_patch(project_root, id, path_kind, gcc_append))
    .or_else(|| meson_path_patch(project_root, id, path_kind, gcc_append))
    .or_else(|| cargo_path_patch(project_root, id, path_kind, rust_append))
}

fn source_date_epoch_project_patch(
    project_root: &Path,
    config: &Config,
    id: &str,
    value: &str,
) -> Option<ProjectPatch> {
    let hint = build_command_hint(config);
    if hint.contains("cargo")
        && project_root.join("Cargo.toml").is_file()
        && !project_root.join(".cargo/config").exists()
        && !project_root.join(".cargo/config.toml").exists()
    {
        return create_file_patch(
            project_root,
            PathBuf::from(".cargo/config.toml"),
            format!(
                "# ReproBisect candidate: {id}\n[env]\nSOURCE_DATE_EPOCH = {{ value = {:?}, force = true }}\n",
                value
            ),
        );
    }
    makefile_append_patch(
        project_root,
        id,
        &format!(
            "# ReproBisect candidate: {id}\n# Pin deterministic build epoch\noverride SOURCE_DATE_EPOCH := {value}\nexport SOURCE_DATE_EPOCH\n"
        ),
    )
}

fn apply_project_patch(root: &Path, patch: &ProjectPatch) -> Result<()> {
    let path = root.join(&patch.path);
    match &patch.operation {
        ProjectPatchOperation::Create { text } => {
            if fs::symlink_metadata(&path).is_ok() {
                anyhow::bail!(
                    "project patch target {} now exists; refusing to overwrite it",
                    patch.path.display()
                );
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("cannot create project patch directory {}", parent.display())
                })?;
            }
            fs::write(&path, text)
                .with_context(|| format!("cannot create project patch target {}", path.display()))?;
        }
        ProjectPatchOperation::Append { text } => {
            verify_project_patch_precondition(&path, patch)?;
            let mut file = OpenOptions::new()
                .append(true)
                .open(&path)
                .with_context(|| format!("cannot open project patch target {}", path.display()))?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
        }
        ProjectPatchOperation::InsertAfterLine { line_number, text } => {
            verify_project_patch_precondition(&path, patch)?;
            let original = fs::read_to_string(&path)
                .with_context(|| format!("cannot read project patch target {}", path.display()))?;
            let insertion_offset = insertion_offset_after_line(&original, *line_number).with_context(|| {
                format!("project patch line {} is no longer valid for {}", line_number, patch.path.display())
            })?;
            let mut updated = String::with_capacity(original.len() + text.len() + 1);
            updated.push_str(&original[..insertion_offset]);
            if insertion_offset == original.len() && !original.ends_with('\n') {
                updated.push('\n');
            }
            updated.push_str(text);
            updated.push_str(&original[insertion_offset..]);
            fs::write(&path, updated)
                .with_context(|| format!("cannot write project patch target {}", path.display()))?;
        }
        ProjectPatchOperation::ReplaceText { expected, replacement } => {
            verify_project_patch_precondition(&path, patch)?;
            let original = fs::read_to_string(&path)
                .with_context(|| format!("cannot read project patch target {}", path.display()))?;
            if original.match_indices(expected).count() != 1 {
                anyhow::bail!(
                    "project patch target {} no longer contains exactly one expected replacement site",
                    patch.path.display()
                );
            }
            let updated = original.replacen(expected, replacement, 1);
            fs::write(&path, updated)
                .with_context(|| format!("cannot write project patch target {}", path.display()))?;
        }
    }
    Ok(())
}

fn verify_project_patch_precondition(path: &Path, patch: &ProjectPatch) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("cannot stat project patch target {}", path.display()))?;
    if !metadata.file_type().is_file() {
        anyhow::bail!("project patch target {} is not a regular file", patch.path.display());
    }
    let expected = patch
        .expected_sha256
        .as_deref()
        .context("existing-file patch is missing expected_sha256")?;
    let actual_sha256 = sha256_file(path)?;
    if actual_sha256 != expected {
        anyhow::bail!(
            "project patch target {} changed (expected sha256 {}, got {}); refusing to apply stale patch",
            patch.path.display(),
            expected,
            actual_sha256
        );
    }
    Ok(())
}

fn insertion_offset_after_line(text: &str, line_number: usize) -> Option<usize> {
    if line_number == 0 {
        return Some(0);
    }
    let mut lines_seen = 0_usize;
    for (index, ch) in text.char_indices() {
        if ch == '\n' {
            lines_seen += 1;
            if lines_seen == line_number {
                return Some(index + 1);
            }
        }
    }
    if lines_seen + 1 == line_number && !text.is_empty() {
        Some(text.len())
    } else {
        None
    }
}

fn append_flags(existing: &str, addition: &str) -> String {
    let existing = existing.trim();
    if existing.is_empty() {
        addition.to_string()
    } else {
        format!("{existing} {addition}")
    }
}

fn verify_plan(
    project_root: &Path,
    config: &Config,
    diagnosis: &CheckReport,
    plan: &FixPlan,
) -> FixVerification {
    if !plan.automatic_verification_supported {
        return FixVerification {
            plan_id: plan.id.clone(),
            experiment_id: None,
            attempted: false,
            verified: false,
            baseline_runs: Vec::new(),
            intervention_runs: Vec::new(),
            evidence: vec![
                "verification was not attempted because no semantics-preserving automatic flag injection point was available"
                    .to_string(),
            ],
            error: None,
        };
    }

    match verify_plan_inner(project_root, config, diagnosis, plan) {
        Ok(verification) => verification,
        Err(error) => FixVerification {
            plan_id: plan.id.clone(),
            experiment_id: None,
            attempted: true,
            verified: false,
            baseline_runs: Vec::new(),
            intervention_runs: Vec::new(),
            evidence: Vec::new(),
            error: Some(format!("{error:#}")),
        },
    }
}

fn verify_plan_inner(
    project_root: &Path,
    config: &Config,
    diagnosis: &CheckReport,
    plan: &FixPlan,
) -> Result<FixVerification> {
    let patch_workspace = if plan.project_patch.is_some() {
        let temp = tempfile::Builder::new()
            .prefix("reprobisect-fix-verify-")
            .tempdir()
            .context("cannot create temporary project-patch verification tree")?;
        copy_source_tree(project_root, temp.path(), SourceCopyOrder::Sorted)?;
        apply_project_patch(
            temp.path(),
            plan.project_patch
                .as_ref()
                .context("project patch disappeared during verification")?,
        )?;
        Some(temp)
    } else {
        None
    };
    let verification_root = patch_workspace
        .as_ref()
        .map(|temp| temp.path())
        .unwrap_or(project_root);
    let verification_config = if patch_workspace.is_some() {
        Config::load(verification_root).context("patched project configuration is invalid")?
    } else {
        config.clone()
    };

    let source_digest = digest_tree(verification_root).context("failed to digest verification source tree")?;
    if plan.project_patch.is_none() && source_digest != diagnosis.source_digest {
        anyhow::bail!(
            "source tree changed after diagnosis (diagnosed {}, now {}); refusing to verify against a different source snapshot",
            diagnosis.source_digest,
            source_digest
        );
    }

    let mut environment = verification_config.build.env.clone();
    if plan.project_patch.is_none() {
        for (key, addition) in &plan.suggested_environment_appends {
            if let Some(existing) = verification_config.build.env.get(key) {
                environment.insert(key.clone(), append_flags(existing, addition));
            }
        }
    }
    let spec = BuildSpec {
        image: verification_config.build.image.clone(),
        command: verification_config.build.command.clone(),
        outputs: verification_config.build.outputs.clone(),
        environment,
        working_directory: verification_config.build.working_directory.clone(),
        timeout_seconds: verification_config.build.timeout_seconds,
        log_capture_max_bytes: verification_config.build.log_capture_max_bytes,
    };

    let mut baseline = baseline_environment(&verification_config);
    let mut planned = plan_interventions(&verification_config, &baseline)
        .into_iter()
        .find(|candidate| {
            candidate.intervention.variable.as_str() == plan.causal_variable.as_str()
        })
        .with_context(|| {
            format!(
                "cannot recover original intervention for {}",
                plan.causal_variable
            )
        })?;

    if plan.project_patch.is_none() {
        apply_plan_to_environments(plan, &mut baseline, &mut planned.environment)?;
    }

    let runner = configured_runner(verification_root, verification_config.build.runner);
    runner.available()?;
    let experiment_id = Uuid::new_v4();
    let mut ordinal = 1_usize;
    let mut baseline_runs = Vec::new();
    let mut intervention_runs = Vec::new();

    for _ in 0..VERIFICATION_RUNS {
        let run = runner.run(
            &spec,
            experiment_id,
            &source_digest,
            ordinal,
            &baseline,
        )?;
        persist_run(project_root, &run)?;
        baseline_runs.push(run);
        ordinal += 1;
    }
    for _ in 0..VERIFICATION_RUNS {
        let run = runner.run(
            &spec,
            experiment_id,
            &source_digest,
            ordinal,
            &planned.environment,
        )?;
        persist_run(project_root, &run)?;
        intervention_runs.push(run);
        ordinal += 1;
    }

    let baseline_stable = runs_stable(&baseline_runs);
    let intervention_stable = runs_stable(&intervention_runs);
    let intervention_matches_baseline = baseline_runs.first().is_some_and(|baseline_run| {
        intervention_runs
            .iter()
            .all(|run| same_artifact_hashes(baseline_run, run))
    });
    let verified = baseline_stable && intervention_stable && intervention_matches_baseline;

    let mut evidence = vec![format!(
        "fixed baseline was byte-stable across {} run(s): {}",
        baseline_runs.len(),
        baseline_stable
    )];
    if let Some(patch) = &plan.project_patch {
        evidence.push(format!(
            "verification applied the candidate project patch to temporary {} only; the original source tree was not modified",
            patch.path.display()
        ));
    } else {
        evidence.push("verification used an operational normalization without modifying project files".to_string());
    }
    evidence.push(format!(
        "fixed intervention was byte-stable across {} run(s): {}",
        intervention_runs.len(),
        intervention_stable
    ));
    evidence.push(format!(
        "the {} causal dimension, replayed through the proposed normalization, produced the same declared artifact bytes as the fixed baseline: {}",
        plan.causal_variable, intervention_matches_baseline
    ));

    Ok(FixVerification {
        plan_id: plan.id.clone(),
        experiment_id: Some(experiment_id),
        attempted: true,
        verified,
        baseline_runs,
        intervention_runs,
        evidence,
        error: None,
    })
}

fn apply_plan_to_environments(
    plan: &FixPlan,
    baseline: &mut ControlledEnvironment,
    intervention: &mut ControlledEnvironment,
) -> Result<()> {
    match plan.strategy.as_str() {
        "compiler-prefix-map" => {}
        "pin-source-date-epoch" => {
            let value = plan
                .suggested_environment_overrides
                .get("SOURCE_DATE_EPOCH")
                .context("timestamp fix is missing SOURCE_DATE_EPOCH override")?
                .clone();
            baseline
                .environment
                .insert("SOURCE_DATE_EPOCH".to_string(), value.clone());
            intervention
                .environment
                .insert("SOURCE_DATE_EPOCH".to_string(), value);
        }
        "normalize-source-mtime" => {
            let value = plan
                .suggested_runner_controls
                .get("source_mtime_epoch")
                .context("source-mtime fix is missing source_mtime_epoch control")?
                .parse::<i64>()
                .context("invalid source_mtime_epoch in fix plan")?;
            baseline.source_mtime_epoch = Some(value);
            intervention.source_mtime_epoch = Some(value);
        }
        "pin-umask" => {
            let raw = plan
                .suggested_runner_controls
                .get("umask")
                .context("umask fix is missing umask control")?;
            let value = u32::from_str_radix(raw, 8).context("invalid octal umask in fix plan")?;
            baseline.umask = Some(value);
            intervention.umask = Some(value);
        }
        strategy => anyhow::bail!("unsupported automatic fix strategy {strategy:?}"),
    }
    Ok(())
}

fn runs_stable(runs: &[BuildRun]) -> bool {
    let Some(first) = runs.first() else {
        return false;
    };
    runs.iter().skip(1).all(|run| same_artifact_hashes(first, run))
}

fn same_artifact_hashes(left: &BuildRun, right: &BuildRun) -> bool {
    if left.artifacts.len() != right.artifacts.len() {
        return false;
    }
    left.artifacts.iter().all(|left_artifact| {
        right
            .artifacts
            .iter()
            .find(|right_artifact| right_artifact.logical_path == left_artifact.logical_path)
            .is_some_and(|right_artifact| right_artifact.sha256 == left_artifact.sha256)
    })
}

fn persist_fix_report(project_root: &Path, report: &FixReport) -> Result<PathBuf> {
    let directory = project_root.join(".reprobisect").join("fixes");
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create fix evidence directory {}", directory.display()))?;
    let path = directory.join(format!("{}.json", report.diagnosis.experiment_id));
    let bytes = serde_json::to_vec_pretty(report).context("cannot serialize fix report")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| format!("cannot create immutable fix report {}", path.display()))?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_flags_preserves_existing_flags() {
        assert_eq!(append_flags("-O2", "-fdebug-prefix-map=/a=."), "-O2 -fdebug-prefix-map=/a=.");
    }

    #[test]
    fn path_fix_requires_baseline_and_variant_elf_evidence() {
        use crate::model::{ArtifactDelta, ElfMarkerLocation, Intervention, InterventionResult};

        let mut result = InterventionResult {
            intervention: Intervention {
                id: "build-path".into(),
                kind: InterventionKind::BuildPath,
                variable: "build_path".into(),
                baseline_value: "/workspace".into(),
                variant_value: "/opt/reprobisect/build".into(),
                description: String::new(),
            },
            runs: vec![],
            reference_runs: vec![],
            build_failures: vec![],
            artifact_deltas: vec![ArtifactDelta {
                logical_path: "build/app".into(),
                baseline_sha256: "a".into(),
                variant_sha256: "b".into(),
                changed: true,
                marker_evidence: vec![],
                elf_debug_marker_evidence: vec![],
                elf_marker_location_evidence: vec![ElfMarkerLocation {
                    variable: "build_path".into(),
                    value: "/workspace".into(),
                    section: ".debug_line_str".into(),
                }],
                archive_metadata_evidence: vec![],
                semantic_evidence: vec![],
                baseline_direct_marker: true,
                variant_direct_marker: false,
                direct_structural_evidence: false,
            }],
            changed: true,
            variant_stable: Some(true),
            distinct_variant_outcomes: 1,
            stochastic_effect: None,
            reverted_to_baseline: Some(true),
            confirmation_run: None,
            error: None,
        };

        assert!(!has_path_elf_pair(&result));
        result.artifact_deltas[0]
            .elf_marker_location_evidence
            .push(ElfMarkerLocation {
                variable: "build_path".into(),
                value: "/opt/reprobisect/build".into(),
                section: ".debug_line_str".into(),
            });
        assert!(has_path_elf_pair(&result));
    }

    #[test]
    fn makefile_patch_is_append_only_and_rejects_stale_target() {
        let temp = tempfile::tempdir().unwrap();
        let makefile = temp.path().join("Makefile");
        fs::write(&makefile, "all:\n\t@echo ok\n").unwrap();

        let patch = makefile_append_patch(
            temp.path(),
            "test-plan",
            "# ReproBisect candidate: test-plan\nEXTRA_FLAGS := -ffile-prefix-map=/a=.\n",
        )
        .expect("expected root Makefile patch");
        let before = fs::read_to_string(&makefile).unwrap();
        apply_project_patch(temp.path(), &patch).unwrap();
        let after = fs::read_to_string(&makefile).unwrap();
        assert!(after.starts_with(&before));
        assert!(after.contains("# ReproBisect candidate: test-plan"));
        assert!(patch.unified_diff.contains("+++ b/Makefile"));

        fs::write(&makefile, "changed by user\n").unwrap();
        let error = apply_project_patch(temp.path(), &patch).unwrap_err();
        assert!(format!("{error:#}").contains("refusing to apply stale patch"));
    }


    #[test]
    fn build_system_specific_path_patches_are_minimal_and_applicable() {
        let gcc = "-ffile-prefix-map=/workspace=. -fdebug-prefix-map=/workspace=.";
        let rust = "--remap-path-prefix=/workspace=.";

        let cmake = tempfile::tempdir().unwrap();
        fs::write(
            cmake.path().join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.20)\nproject(demo C)\nadd_executable(demo main.c)\n",
        )
        .unwrap();
        let cmake_config: Config = toml::from_str(
            r#"
            [build]
            image = "gcc:14"
            command = ["cmake", "-S", ".", "-B", "build"]
            outputs = ["build/demo"]
            "#,
        )
        .unwrap();
        let patch = path_project_patch(cmake.path(), &cmake_config, "cmake-test", "build", gcc, rust)
            .expect("expected CMake patch");
        assert_eq!(patch.path, PathBuf::from("CMakeLists.txt"));
        assert!(matches!(&patch.operation, ProjectPatchOperation::Append { .. }));
        apply_project_patch(cmake.path(), &patch).unwrap();
        let updated = fs::read_to_string(cmake.path().join("CMakeLists.txt")).unwrap();
        assert!(updated.contains("CMAKE_C_FLAGS"));
        assert!(updated.contains("# ReproBisect candidate: cmake-test"));

        let meson = tempfile::tempdir().unwrap();
        fs::write(
            meson.path().join("meson.build"),
            "project(\n  'demo',\n  'c',\n)\nexecutable('demo', 'main.c')\n",
        )
        .unwrap();
        let meson_config: Config = toml::from_str(
            r#"
            [build]
            image = "gcc:14"
            command = ["meson", "setup", "build"]
            outputs = ["build/demo"]
            "#,
        )
        .unwrap();
        let patch = path_project_patch(meson.path(), &meson_config, "meson-test", "build", gcc, rust)
            .expect("expected Meson patch");
        assert_eq!(patch.path, PathBuf::from("meson.build"));
        assert!(matches!(&patch.operation, ProjectPatchOperation::InsertAfterLine { .. }));
        apply_project_patch(meson.path(), &patch).unwrap();
        let updated = fs::read_to_string(meson.path().join("meson.build")).unwrap();
        assert!(updated.find("add_project_arguments").unwrap() < updated.find("executable(").unwrap());

        let cargo = tempfile::tempdir().unwrap();
        fs::write(
            cargo.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let cargo_config: Config = toml::from_str(
            r#"
            [build]
            image = "rust:1.85-bookworm"
            command = ["cargo", "build"]
            outputs = ["target/debug/demo"]
            "#,
        )
        .unwrap();
        let patch = path_project_patch(cargo.path(), &cargo_config, "cargo-test", "build", gcc, rust)
            .expect("expected Cargo patch");
        assert_eq!(patch.path, PathBuf::from(".cargo/config.toml"));
        assert!(matches!(&patch.operation, ProjectPatchOperation::Create { .. }));
        apply_project_patch(cargo.path(), &patch).unwrap();
        let updated = fs::read_to_string(cargo.path().join(".cargo/config.toml")).unwrap();
        assert!(updated.contains("--remap-path-prefix=/workspace=."));
    }

    #[test]
    fn make_archive_patches_use_tar_options_without_touching_recipe_lines() {
        let temp = tempfile::tempdir().unwrap();
        let makefile = temp.path().join("Makefile");
        let original = ".PHONY: all\nall:\n\ttar -cf build/out.tar payload.txt\n";
        fs::write(&makefile, original).unwrap();
        let config: Config = toml::from_str(
            r#"
            [build]
            image = "debian:bookworm"
            command = ["make"]
            outputs = ["build/out.tar"]
            "#,
        )
        .unwrap();

        let mtime = archive_mtime_project_patch(temp.path(), &config, "mtime-test", "946684800")
            .expect("expected archive mtime patch");
        assert_eq!(mtime.path, PathBuf::from("Makefile"));
        apply_project_patch(temp.path(), &mtime).unwrap();
        let updated = fs::read_to_string(&makefile).unwrap();
        assert!(updated.starts_with(original));
        assert!(updated.contains("--mtime=@$(REPROBISECT_ARCHIVE_EPOCH)"));
        assert!(updated.contains("--clamp-mtime"));

        fs::write(&makefile, original).unwrap();
        let mode = archive_mode_project_patch(temp.path(), &config, "mode-test")
            .expect("expected archive mode patch");
        apply_project_patch(temp.path(), &mode).unwrap();
        let updated = fs::read_to_string(&makefile).unwrap();
        assert!(updated.starts_with(original));
        assert!(updated.contains("--mode=u+rwX,go+rX,go-w"));
    }

    #[test]
    fn cmake_meson_and_shell_archive_patches_are_single_site_and_applicable() {
        let cmake = tempfile::tempdir().unwrap();
        fs::write(
            cmake.path().join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.20)\nproject(pkg NONE)\nadd_custom_target(package ALL COMMAND tar -cf ${CMAKE_BINARY_DIR}/out.tar payload.txt)\n",
        )
        .unwrap();
        let cmake_config: Config = toml::from_str(
            r#"
            [build]
            image = "debian:bookworm"
            command = ["cmake", "-S", ".", "-B", "build"]
            outputs = ["build/out.tar"]
            "#,
        )
        .unwrap();
        let patch = archive_mtime_project_patch(cmake.path(), &cmake_config, "cmake-mtime", "946684800")
            .expect("expected one-site CMake archive patch");
        assert!(matches!(&patch.operation, ProjectPatchOperation::ReplaceText { .. }));
        apply_project_patch(cmake.path(), &patch).unwrap();
        let updated = fs::read_to_string(cmake.path().join("CMakeLists.txt")).unwrap();
        assert!(updated.contains("${CMAKE_COMMAND} -E env"));
        assert!(updated.contains("TAR_OPTIONS=--mtime=@946684800 --clamp-mtime"));

        let meson = tempfile::tempdir().unwrap();
        fs::write(
            meson.path().join("meson.build"),
            "project('pkg')\nrun_command('tar', '-cf', 'out.tar', 'payload.txt')\n",
        )
        .unwrap();
        let meson_config: Config = toml::from_str(
            r#"
            [build]
            image = "debian:bookworm"
            command = ["meson", "setup", "build"]
            outputs = ["build/out.tar"]
            "#,
        )
        .unwrap();
        let patch = archive_mode_project_patch(meson.path(), &meson_config, "meson-mode")
            .expect("expected one-site Meson archive patch");
        assert!(matches!(&patch.operation, ProjectPatchOperation::ReplaceText { .. }));
        apply_project_patch(meson.path(), &patch).unwrap();
        let updated = fs::read_to_string(meson.path().join("meson.build")).unwrap();
        assert!(updated.contains("run_command('env', 'TAR_OPTIONS=--mode=u+rwX,go+rX,go-w', 'tar',"));

        let shell = tempfile::tempdir().unwrap();
        fs::write(shell.path().join("package.sh"), "#!/bin/sh\nset -eu\ntar -cf build/out.tar payload.txt\n").unwrap();
        let shell_config: Config = toml::from_str(
            r#"
            [build]
            image = "debian:bookworm"
            command = ["sh", "-lc", "./package.sh"]
            outputs = ["build/out.tar"]
            "#,
        )
        .unwrap();
        let patch = archive_mtime_project_patch(shell.path(), &shell_config, "shell-mtime", "946684800")
            .expect("expected one-site shell archive patch");
        assert_eq!(patch.path, PathBuf::from("package.sh"));
        apply_project_patch(shell.path(), &patch).unwrap();
        let updated = fs::read_to_string(shell.path().join("package.sh")).unwrap();
        assert!(updated.contains("TAR_OPTIONS='--mtime=@946684800 --clamp-mtime' tar -cf"));
    }

    #[test]
    fn archive_patch_refuses_ambiguous_cmake_and_meson_sites() {
        let cmake = tempfile::tempdir().unwrap();
        fs::write(
            cmake.path().join("CMakeLists.txt"),
            "project(pkg NONE)\nadd_custom_target(a COMMAND tar -cf a.tar a)\nadd_custom_target(b COMMAND /usr/bin/tar -cf b.tar b)\n",
        )
        .unwrap();
        let cmake_config: Config = toml::from_str(
            r#"
            [build]
            image = "debian:bookworm"
            command = ["cmake", "-S", ".", "-B", "build"]
            outputs = ["build/a.tar"]
            "#,
        )
        .unwrap();
        assert!(archive_mtime_project_patch(cmake.path(), &cmake_config, "ambiguous", "1").is_none());

        let meson = tempfile::tempdir().unwrap();
        fs::write(
            meson.path().join("meson.build"),
            "project('pkg')\nrun_command('tar', '-cf', 'a.tar', 'a')\nrun_command(\"tar\", '-cf', 'b.tar', 'b')\n",
        )
        .unwrap();
        let meson_config: Config = toml::from_str(
            r#"
            [build]
            image = "debian:bookworm"
            command = ["meson", "setup", "build"]
            outputs = ["build/a.tar"]
            "#,
        )
        .unwrap();
        assert!(archive_mode_project_patch(meson.path(), &meson_config, "ambiguous").is_none());
    }

    #[test]
    fn operational_fix_transforms_neutralize_the_original_dimension() {
        let mut baseline = ControlledEnvironment::default();
        baseline.environment.insert("SOURCE_DATE_EPOCH".into(), "1".into());
        baseline.source_mtime_epoch = Some(1);
        baseline.umask = Some(0o022);
        let mut variant = baseline.clone();
        variant.environment.insert("SOURCE_DATE_EPOCH".into(), "2".into());
        variant.source_mtime_epoch = Some(2);
        variant.umask = Some(0o077);

        let mut overrides = BTreeMap::new();
        overrides.insert("SOURCE_DATE_EPOCH".into(), "1".into());
        let timestamp = FixPlan {
            id: "timestamp".into(), title: String::new(), causal_variable: "SOURCE_DATE_EPOCH".into(),
            strategy: "pin-source-date-epoch".into(), suggested_environment_appends: BTreeMap::new(),
            suggested_environment_overrides: overrides, suggested_runner_controls: BTreeMap::new(),
            suggested_shell_commands: vec![], project_patch: None, automatic_verification_supported: true, evidence: vec![], limitations: vec![],
        };
        apply_plan_to_environments(&timestamp, &mut baseline, &mut variant).unwrap();
        assert_eq!(baseline.environment["SOURCE_DATE_EPOCH"], variant.environment["SOURCE_DATE_EPOCH"]);

        let mut mtime_controls = BTreeMap::new();
        mtime_controls.insert("source_mtime_epoch".into(), "1".into());
        let mtime = FixPlan {
            id: "mtime".into(), title: String::new(), causal_variable: "source_mtime".into(),
            strategy: "normalize-source-mtime".into(), suggested_environment_appends: BTreeMap::new(),
            suggested_environment_overrides: BTreeMap::new(), suggested_runner_controls: mtime_controls,
            suggested_shell_commands: vec![], project_patch: None, automatic_verification_supported: true, evidence: vec![], limitations: vec![],
        };
        apply_plan_to_environments(&mtime, &mut baseline, &mut variant).unwrap();
        assert_eq!(baseline.source_mtime_epoch, Some(1));
        assert_eq!(variant.source_mtime_epoch, Some(1));

        let mut controls = BTreeMap::new();
        controls.insert("umask".into(), "022".into());
        let umask = FixPlan {
            id: "umask".into(), title: String::new(), causal_variable: "umask".into(),
            strategy: "pin-umask".into(), suggested_environment_appends: BTreeMap::new(),
            suggested_environment_overrides: BTreeMap::new(), suggested_runner_controls: controls,
            suggested_shell_commands: vec![], project_patch: None, automatic_verification_supported: true, evidence: vec![], limitations: vec![],
        };
        apply_plan_to_environments(&umask, &mut baseline, &mut variant).unwrap();
        assert_eq!(baseline.umask, Some(0o022));
        assert_eq!(variant.umask, Some(0o022));
    }

}
