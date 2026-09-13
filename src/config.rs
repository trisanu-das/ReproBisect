use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::model::RunnerBackend;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub build: BuildConfig,
    #[serde(default)]
    pub experiments: ExperimentsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    #[serde(default)]
    pub runner: RunnerBackend,
    pub image: String,
    pub command: Vec<String>,
    pub outputs: Vec<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_working_directory")]
    pub working_directory: PathBuf,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
    /// Maximum stdout/stderr bytes retained per stream. The runner still drains
    /// and hashes the complete stream so evidence fingerprints remain exact.
    #[serde(default = "default_log_capture_max_bytes")]
    pub log_capture_max_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyVariantConfig {
    pub id: String,
    pub target: PathBuf,
    pub variant_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentsConfig {
    #[serde(default = "default_control_runs")]
    pub control_runs: usize,
    #[serde(default = "default_intervention_runs")]
    pub intervention_runs: usize,
    #[serde(default = "default_confirmation_runs")]
    pub confirmation_runs: usize,
    #[serde(default = "default_stochastic_runs")]
    pub stochastic_runs: usize,
    /// Significance threshold for the fixed-sample matched stochastic probe.
    #[serde(default = "default_stochastic_alpha")]
    pub stochastic_alpha: f64,
    #[serde(default = "default_interaction_runs")]
    pub interaction_runs: usize,
    #[serde(default = "default_comparison_runs")]
    pub comparison_runs: usize,
    #[serde(default = "default_comparison_subset_runs")]
    pub comparison_subset_runs: usize,
    #[serde(default = "default_max_interaction_variables")]
    pub max_interaction_variables: usize,
    #[serde(default)]
    pub interaction_search: bool,
    #[serde(default)]
    pub dimensions: ExperimentDimensionsConfig,
    /// Alternative container/build images used as coarse-grained toolchain interventions.
    #[serde(default)]
    pub image_variants: Vec<String>,
    /// Narrow compiler/linker/archive-tool bindings varied within the same image.
    /// Keys are restricted to well-known toolchain environment variables and each
    /// value must be [baseline_executable, variant_executable].
    #[serde(default)]
    pub toolchain_variables: BTreeMap<String, Vec<String>>,
    /// Replace exactly one dependency declaration/lock file in the fresh source
    /// snapshot with another project-relative file. The project tree is untouched.
    #[serde(default)]
    pub dependency_variants: Vec<DependencyVariantConfig>,
    /// Best-effort strace-based network syscall provenance. Raw traces are discarded.
    #[serde(default)]
    pub network_trace: bool,
    /// Best-effort process-aware file syscall provenance. Raw paths and traces are discarded.
    #[serde(default)]
    pub file_input_trace: bool,
    /// Maximum raw syscall-trace bytes parsed after the build. This bounds parser
    /// memory/work, not the temporary strace file's disk growth.
    #[serde(default = "default_syscall_trace_max_bytes")]
    pub syscall_trace_max_bytes: u64,
    /// Collect post-build lockfile/resolver state and aggregate downloaded-cache
    /// content fingerprints. Package names/cache paths are not persisted.
    #[serde(default)]
    pub runtime_dependency_provenance: bool,
    /// Additional in-container package-manager cache roots to summarize.
    #[serde(default)]
    pub dependency_cache_paths: BTreeMap<String, String>,
    /// Per-cache content-hashing bound. Directory enumeration is best-effort,
    /// but ReproBisect will not hash more than this many files per cache root.
    #[serde(default = "default_dependency_cache_max_files")]
    pub dependency_cache_max_files: usize,
    /// Per-cache cumulative byte bound for content hashing.
    #[serde(default = "default_dependency_cache_max_bytes")]
    pub dependency_cache_max_bytes: u64,
    #[serde(default)]
    pub environment_variables: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentDimensionsConfig {
    #[serde(default)]
    pub network_access: bool,
    #[serde(default)]
    pub source_path: bool,
    #[serde(default = "default_true")]
    pub build_path: bool,
    #[serde(default = "default_true")]
    pub source_date_epoch: bool,
    #[serde(default = "default_true")]
    pub timezone: bool,
    #[serde(default = "default_true")]
    pub locale: bool,
    #[serde(default = "default_true")]
    pub hostname: bool,
    #[serde(default)]
    pub source_mtime: bool,
    #[serde(default)]
    pub cpu_count: bool,
    #[serde(default)]
    pub umask: bool,
    #[serde(default)]
    pub directory_order: bool,
}

impl Default for ExperimentsConfig {
    fn default() -> Self {
        Self {
            control_runs: default_control_runs(),
            intervention_runs: default_intervention_runs(),
            confirmation_runs: default_confirmation_runs(),
            stochastic_runs: default_stochastic_runs(),
            stochastic_alpha: default_stochastic_alpha(),
            interaction_runs: default_interaction_runs(),
            comparison_runs: default_comparison_runs(),
            comparison_subset_runs: default_comparison_subset_runs(),
            max_interaction_variables: default_max_interaction_variables(),
            interaction_search: false,
            dimensions: ExperimentDimensionsConfig::default(),
            image_variants: Vec::new(),
            toolchain_variables: BTreeMap::new(),
            dependency_variants: Vec::new(),
            network_trace: false,
            file_input_trace: false,
            syscall_trace_max_bytes: default_syscall_trace_max_bytes(),
            runtime_dependency_provenance: false,
            dependency_cache_paths: BTreeMap::new(),
            dependency_cache_max_files: default_dependency_cache_max_files(),
            dependency_cache_max_bytes: default_dependency_cache_max_bytes(),
            environment_variables: BTreeMap::new(),
        }
    }
}

impl Default for ExperimentDimensionsConfig {
    fn default() -> Self {
        Self {
            network_access: false,
            source_path: false,
            build_path: true,
            source_date_epoch: true,
            timezone: true,
            locale: true,
            hostname: true,
            source_mtime: false,
            cpu_count: false,
            umask: false,
            directory_order: false,
        }
    }
}

impl Config {
    pub fn load(project_root: &Path) -> Result<Self> {
        let path = project_root.join(".reprobisect.toml");
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}; run `reprobisect init` first", path.display()))?;
        let config: Config = toml::from_str(&raw)
            .with_context(|| format!("invalid configuration in {}", path.display()))?;
        config.validate(project_root)?;
        Ok(config)
    }

    fn validate(&self, project_root: &Path) -> Result<()> {
        if self.build.image.trim().is_empty() {
            bail!("build.image cannot be empty");
        }
        if self.build.command.is_empty() {
            bail!("build.command must contain at least one argv element");
        }
        if self.build.outputs.is_empty() {
            bail!("build.outputs must declare at least one artifact");
        }
        if !(2..=32).contains(&self.experiments.control_runs) {
            bail!("experiments.control_runs must be between 2 and 32");
        }
        if !(1..=32).contains(&self.experiments.intervention_runs) {
            bail!("experiments.intervention_runs must be between 1 and 32");
        }
        if self.experiments.confirmation_runs > 1 {
            bail!("experiments.confirmation_runs currently supports only 0 or 1");
        }
        if self.experiments.stochastic_runs < 3 {
            bail!("experiments.stochastic_runs must be at least 3");
        }
        if self.experiments.stochastic_runs > 1024 {
            bail!("experiments.stochastic_runs must not exceed 1024");
        }
        if !(0.0 < self.experiments.stochastic_alpha
            && self.experiments.stochastic_alpha < 1.0)
        {
            bail!("experiments.stochastic_alpha must be strictly between 0 and 1");
        }
        if !(2..=32).contains(&self.experiments.interaction_runs) {
            bail!("experiments.interaction_runs must be between 2 and 32");
        }
        if !(2..=32).contains(&self.experiments.comparison_runs) {
            bail!("experiments.comparison_runs must be between 2 and 32");
        }
        if !(2..=32).contains(&self.experiments.comparison_subset_runs) {
            bail!("experiments.comparison_subset_runs must be between 2 and 32");
        }
        if !(2..=32).contains(&self.experiments.max_interaction_variables) {
            bail!("experiments.max_interaction_variables must be between 2 and 32");
        }
        if self.build.timeout_seconds == 0 {
            bail!("build.timeout_seconds must be greater than 0");
        }
        if !(64 * 1024..=16 * 1024 * 1024).contains(&self.build.log_capture_max_bytes) {
            bail!("build.log_capture_max_bytes must be between 65536 and 16777216 bytes");
        }

        for output in &self.build.outputs {
            validate_relative_path("build.outputs", output)?;
        }
        validate_relative_path("build.working_directory", &self.build.working_directory)?;

        if self.experiments.image_variants.len() > 4 {
            bail!("experiments.image_variants currently supports at most 4 alternative images");
        }
        let mut seen_images = std::collections::BTreeSet::new();
        for image in &self.experiments.image_variants {
            if image.trim().is_empty() {
                bail!("experiments.image_variants cannot contain an empty image reference");
            }
            if image == &self.build.image {
                bail!("experiments.image_variants must differ from build.image");
            }
            if !seen_images.insert(image) {
                bail!("experiments.image_variants contains duplicate image {image:?}");
            }
        }

        for (key, values) in &self.experiments.toolchain_variables {
            if !matches!(key.as_str(), "CC" | "CXX" | "LD" | "AR" | "RANLIB" | "RUSTC") {
                bail!("experiments.toolchain_variables contains unsupported binding {key:?}; supported keys: CC, CXX, LD, AR, RANLIB, RUSTC");
            }
            if values.len() != 2 {
                bail!("experiments.toolchain_variables.{key} must contain exactly two executable values: [baseline, variant]");
            }
            if values[0] == values[1] {
                bail!("experiments.toolchain_variables.{key} baseline and variant executables must differ");
            }
            for value in values {
                if !valid_toolchain_executable(value) {
                    bail!("experiments.toolchain_variables.{key} contains unsafe executable value {value:?}; use a single executable path/name with optional {{source}}/{{build}} placeholders");
                }
            }
        }

        if self.experiments.dependency_variants.len() > 8 {
            bail!("experiments.dependency_variants currently supports at most 8 variants");
        }
        let mut dependency_ids = std::collections::BTreeSet::new();
        let mut dependency_targets = std::collections::BTreeSet::new();
        for variant in &self.experiments.dependency_variants {
            if variant.id.trim().is_empty() {
                bail!("experiments.dependency_variants id cannot be empty");
            }
            if !dependency_ids.insert(variant.id.as_str()) {
                bail!("experiments.dependency_variants contains duplicate id {:?}", variant.id);
            }
            validate_relative_path("experiments.dependency_variants.target", &variant.target)?;
            validate_relative_path("experiments.dependency_variants.variant_file", &variant.variant_file)?;
            if variant.target == variant.variant_file {
                bail!("dependency variant {:?} target and variant_file must differ", variant.id);
            }
            if !dependency_targets.insert(variant.target.clone()) {
                bail!("multiple dependency variants target {}; use one controlled replacement per target", variant.target.display());
            }
            validate_regular_project_file(project_root, "dependency target", &variant.target)?;
            validate_regular_project_file(project_root, "dependency variant_file", &variant.variant_file)?;
        }

        if self.experiments.dependency_cache_paths.len() > 16 {
            bail!("experiments.dependency_cache_paths currently supports at most 16 entries");
        }
        for (ecosystem, path) in &self.experiments.dependency_cache_paths {
            if ecosystem.is_empty()
                || !ecosystem
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
            {
                bail!("experiments.dependency_cache_paths contains invalid ecosystem label {ecosystem:?}");
            }
            if !path.starts_with('/')
                || path.chars().any(char::is_whitespace)
                || path.contains('\t')
                || path.contains('\n')
                || path.split('/').any(|component| component == "..")
            {
                bail!("experiments.dependency_cache_paths.{ecosystem} must be a simple absolute in-container path without whitespace or '..'");
            }
        }

        if self.experiments.syscall_trace_max_bytes < 1024 * 1024
            || self.experiments.syscall_trace_max_bytes > 512_u64 * 1024 * 1024
        {
            bail!("experiments.syscall_trace_max_bytes must be between 1 MiB and 512 MiB");
        }

        if self.experiments.dependency_cache_max_files == 0
            || self.experiments.dependency_cache_max_files > 100_000
        {
            bail!("experiments.dependency_cache_max_files must be between 1 and 100000");
        }
        if self.experiments.dependency_cache_max_bytes < 1024 * 1024
            || self.experiments.dependency_cache_max_bytes > 4_u64 * 1024 * 1024 * 1024
        {
            bail!("experiments.dependency_cache_max_bytes must be between 1 MiB and 4 GiB");
        }

        for key in self.experiments.toolchain_variables.keys() {
            if self.experiments.environment_variables.contains_key(key) {
                bail!("experiments.toolchain_variables.{key} conflicts with experiments.environment_variables.{key}; control the binding through only one experiment dimension");
            }
        }

        for (key, values) in &self.experiments.environment_variables {
            if !valid_environment_variable_name(key) {
                bail!("experiments.environment_variables contains invalid variable name {key:?}");
            }
            if matches!(
                key.as_str(),
                "SOURCE_DATE_EPOCH" | "TZ" | "LANG" | "LC_ALL" | "REPROBISECT_CPU_COUNT"
            ) {
                bail!("experiments.environment_variables.{key} conflicts with a built-in intervention dimension");
            }
            if values.len() != 2 {
                bail!("experiments.environment_variables.{key} must contain exactly two values: [baseline, variant]");
            }
            if values[0] == values[1] {
                bail!("experiments.environment_variables.{key} baseline and variant values must differ");
            }
        }

        if !project_root.is_dir() {
            bail!("project root {} is not a directory", project_root.display());
        }
        Ok(())
    }
}

fn validate_relative_path(field: &str, path: &Path) -> Result<()> {
    if path.is_absolute() {
        bail!("{field} entries must be relative paths: {}", path.display());
    }
    if path
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        bail!("{field} may not escape the project root: {}", path.display());
    }
    Ok(())
}

fn valid_environment_variable_name(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn validate_regular_project_file(project_root: &Path, label: &str, relative: &Path) -> Result<()> {
    let absolute = project_root.join(relative);
    let metadata = fs::symlink_metadata(&absolute)
        .with_context(|| format!("cannot stat {label} {}", absolute.display()))?;
    if !metadata.file_type().is_file() {
        bail!("{label} {} must be a regular file (symlinks are not followed)", relative.display());
    }
    Ok(())
}

fn valid_toolchain_executable(value: &str) -> bool {
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return false;
    }
    let expanded = value.replace("{source}", "/src").replace("{build}", "/workspace");
    if expanded.contains('{') || expanded.contains('}') {
        return false;
    }
    expanded.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '+' | '.' | '/' | ':')
    })
}

fn default_working_directory() -> PathBuf { PathBuf::from(".") }
fn default_timeout_seconds() -> u64 { 600 }
fn default_log_capture_max_bytes() -> u64 { 1024 * 1024 }
fn default_dependency_cache_max_files() -> usize { 2048 }
fn default_dependency_cache_max_bytes() -> u64 { 128 * 1024 * 1024 }
fn default_syscall_trace_max_bytes() -> u64 { 32 * 1024 * 1024 }

fn default_control_runs() -> usize { 2 }
fn default_intervention_runs() -> usize { 1 }
fn default_confirmation_runs() -> usize { 1 }
fn default_stochastic_runs() -> usize { 4 }
fn default_stochastic_alpha() -> f64 { 0.05 }
fn default_interaction_runs() -> usize { 2 }
fn default_comparison_runs() -> usize { 2 }
fn default_comparison_subset_runs() -> usize { 2 }
fn default_max_interaction_variables() -> usize { 8 }
fn default_true() -> bool { true }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_config() {
        let config: Config = toml::from_str(r#"
            [build]
            image = "gcc:14"
            command = ["sh", "-lc", "make"]
            outputs = ["build/app"]
        "#).unwrap();
        assert_eq!(config.build.runner, RunnerBackend::Docker);
        assert_eq!(config.build.log_capture_max_bytes, 1024 * 1024);
        assert_eq!(config.experiments.control_runs, 2);
        assert_eq!(config.experiments.intervention_runs, 1);
        assert_eq!(config.experiments.confirmation_runs, 1);
        assert_eq!(config.experiments.stochastic_runs, 4);
        assert_eq!(config.experiments.stochastic_alpha, 0.05);
        assert_eq!(config.experiments.interaction_runs, 2);
        assert_eq!(config.experiments.comparison_runs, 2);
        assert_eq!(config.experiments.comparison_subset_runs, 2);
        assert_eq!(config.experiments.max_interaction_variables, 8);
        assert!(!config.experiments.interaction_search);
        assert!(!config.experiments.dimensions.network_access);
        assert!(!config.experiments.dimensions.source_path);
        assert!(config.experiments.dimensions.build_path);
        assert!(config.experiments.dimensions.source_date_epoch);
        assert!(!config.experiments.dimensions.source_mtime);
        assert!(!config.experiments.dimensions.cpu_count);
        assert!(!config.experiments.dimensions.umask);
        assert!(!config.experiments.dimensions.directory_order);
        assert!(config.experiments.image_variants.is_empty());
        assert!(config.experiments.toolchain_variables.is_empty());
        assert!(config.experiments.dependency_variants.is_empty());
        assert!(!config.experiments.network_trace);
        assert!(!config.experiments.file_input_trace);
        assert_eq!(config.experiments.syscall_trace_max_bytes, 32 * 1024 * 1024);
        assert!(!config.experiments.runtime_dependency_provenance);
        assert!(config.experiments.dependency_cache_paths.is_empty());
        assert_eq!(config.build.timeout_seconds, 600);
        assert_eq!(config.build.working_directory, PathBuf::from("."));
    }

    #[test]
    fn rejects_excessive_run_budgets_and_log_capture() {
        let project = tempfile::tempdir().unwrap();
        let base = r#"
            [build]
            image = "debian:bookworm"
            command = ["true"]
            outputs = ["out"]

            [experiments]
            control_runs = 2
        "#;

        let mut config: Config = toml::from_str(base).unwrap();
        config.experiments.control_runs = 33;
        assert!(config.validate(project.path()).unwrap_err().to_string().contains("control_runs"));

        let mut config: Config = toml::from_str(base).unwrap();
        config.experiments.intervention_runs = 33;
        assert!(config.validate(project.path()).unwrap_err().to_string().contains("intervention_runs"));

        let mut config: Config = toml::from_str(base).unwrap();
        config.build.log_capture_max_bytes = 16 * 1024 * 1024 + 1;
        assert!(config.validate(project.path()).unwrap_err().to_string().contains("log_capture_max_bytes"));
    }

    #[test]
    fn parses_podman_runner() {
        let config: Config = toml::from_str(r#"
            [build]
            runner = "podman"
            image = "docker.io/library/gcc:14"
            command = ["true"]
            outputs = ["out"]
        "#).unwrap();
        assert_eq!(config.build.runner, RunnerBackend::Podman);
    }

    #[test]
    fn parses_environment_intervention() {
        let config: Config = toml::from_str(r#"
            [build]
            image = "gcc:14"
            command = ["true"]
            outputs = ["out"]

            [experiments.environment_variables]
            BUILD_FLAVOR = ["alpha", "beta"]
        "#).unwrap();
        assert_eq!(
            config.experiments.environment_variables["BUILD_FLAVOR"],
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    #[test]
    fn parses_toolchain_image_variants() {
        let config: Config = toml::from_str(r#"
            [build]
            image = "gcc:14"
            command = ["true"]
            outputs = ["out"]

            [experiments]
            image_variants = ["gcc:15"]

            [experiments.dimensions]
            network_access = true
        "#).unwrap();
        assert_eq!(config.experiments.image_variants, vec!["gcc:15".to_string()]);
        assert!(config.experiments.dimensions.network_access);
    }

    #[test]
    fn parses_narrow_toolchain_and_dependency_variants() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("requirements.txt"), "demo==1\n").unwrap();
        fs::write(temp.path().join("requirements.variant.txt"), "demo==2\n").unwrap();
        let config: Config = toml::from_str(r#"
            [build]
            image = "gcc:14"
            command = ["true"]
            outputs = ["out"]

            [experiments]
            network_trace = true
            file_input_trace = true
            syscall_trace_max_bytes = 16777216
            runtime_dependency_provenance = true

            [experiments.dependency_cache_paths]
            demo = "/tmp/reprobisect-demo-cache"

            [experiments.toolchain_variables]
            CC = ["gcc", "{source}/toolchains/cc-alt"]

            [[experiments.dependency_variants]]
            id = "requirements-demo"
            target = "requirements.txt"
            variant_file = "requirements.variant.txt"
        "#).unwrap();
        config.validate(temp.path()).unwrap();
        assert_eq!(config.experiments.toolchain_variables["CC"][0], "gcc");
        assert_eq!(config.experiments.dependency_variants.len(), 1);
        assert!(config.experiments.network_trace);
        assert!(config.experiments.file_input_trace);
        assert_eq!(config.experiments.syscall_trace_max_bytes, 16 * 1024 * 1024);
        assert!(config.experiments.runtime_dependency_provenance);
        assert_eq!(
            config.experiments.dependency_cache_paths["demo"],
            "/tmp/reprobisect-demo-cache"
        );
    }

    #[test]
    fn rejects_duplicate_toolchain_and_environment_dimension() {
        let config: Config = toml::from_str(r#"
            [build]
            image = "gcc:14"
            command = ["true"]
            outputs = ["out"]

            [experiments.toolchain_variables]
            CC = ["gcc", "clang"]

            [experiments.environment_variables]
            CC = ["gcc", "clang"]
        "#).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let error = config.validate(temp.path()).unwrap_err();
        assert!(format!("{error:#}").contains("conflicts with experiments.environment_variables.CC"));
    }

    #[test]
    fn rejects_toolchain_shell_fragments() {
        assert!(!valid_toolchain_executable("gcc;curl bad"));
        assert!(!valid_toolchain_executable("$(evil)"));
        assert!(valid_toolchain_executable("{source}/toolchains/cc-alt"));
    }

    #[test]
    fn rejects_parent_output_path() {
        let config: Config = toml::from_str(r#"
            [build]
            image = "gcc:14"
            command = ["make"]
            outputs = ["../escape"]
        "#).unwrap();
        let temp = tempfile::tempdir().unwrap();
        assert!(config.validate(temp.path()).is_err());
    }
}
