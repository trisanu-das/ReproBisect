use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    config::Config,
    model::{ControlledEnvironment, EnvironmentDeltaRecord, SourceCopyOrder},
};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentManifest {
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub build_path: Option<String>,
    #[serde(default)]
    pub source_copy_order: Option<SourceCopyOrder>,
    #[serde(default)]
    pub source_date_epoch: Option<i64>,
    #[serde(default)]
    pub source_mtime_epoch: Option<i64>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub cpu_count: Option<u32>,
    /// Octal file creation mask, for example "022" or "0o077".
    #[serde(default)]
    pub umask: Option<String>,
    /// Currently restricted to "default" or "none" so a comparison cannot
    /// silently attach builds to an arbitrary host/container network.
    #[serde(default)]
    pub network_mode: Option<String>,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    #[serde(default)]
    pub unset_variables: Vec<String>,
    #[serde(default)]
    pub toolchain: BTreeMap<String, String>,
    #[serde(default)]
    pub unset_toolchain: Vec<String>,
    /// Project-relative target -> project-relative variant file.
    #[serde(default)]
    pub dependency_overrides: BTreeMap<PathBuf, PathBuf>,
}

#[derive(Debug, Clone)]
pub struct LoadedEnvironmentManifest {
    pub manifest: EnvironmentManifest,
    pub sha256: String,
}

impl EnvironmentManifest {
    pub fn load(path: &Path, project_root: &Path) -> Result<LoadedEnvironmentManifest> {
        let raw = fs::read(path)
            .with_context(|| format!("cannot read environment manifest {}", path.display()))?;
        let manifest: EnvironmentManifest = toml::from_str(
            std::str::from_utf8(&raw)
                .with_context(|| format!("environment manifest {} is not UTF-8", path.display()))?,
        )
        .with_context(|| format!("invalid environment manifest {}", path.display()))?;
        manifest.validate(project_root)?;
        let mut hasher = Sha256::new();
        hasher.update(&raw);
        Ok(LoadedEnvironmentManifest {
            manifest,
            sha256: hex::encode(hasher.finalize()),
        })
    }

    pub fn apply(
        &self,
        baseline: &ControlledEnvironment,
        config: &Config,
    ) -> Result<ControlledEnvironment> {
        let mut environment = baseline.clone();

        if let Some(image) = &self.image {
            environment.image_override = if image == &config.build.image {
                None
            } else {
                Some(image.clone())
            };
        }
        if let Some(path) = &self.source_path {
            environment.container_source_path = path.clone();
        }
        if let Some(path) = &self.build_path {
            environment.container_work_path = path.clone();
        }
        if let Some(order) = self.source_copy_order {
            environment.source_copy_order = order;
        }
        if let Some(epoch) = self.source_date_epoch {
            environment
                .environment
                .insert("SOURCE_DATE_EPOCH".to_string(), epoch.to_string());
        }
        if let Some(epoch) = self.source_mtime_epoch {
            environment.source_mtime_epoch = Some(epoch);
        }
        if let Some(timezone) = &self.timezone {
            environment
                .environment
                .insert("TZ".to_string(), timezone.clone());
        }
        if let Some(locale) = &self.locale {
            environment
                .environment
                .insert("LANG".to_string(), locale.clone());
            environment
                .environment
                .insert("LC_ALL".to_string(), locale.clone());
        }
        if let Some(hostname) = &self.hostname {
            environment.hostname = Some(hostname.clone());
        }
        if let Some(cpu_count) = self.cpu_count {
            environment.cpu_count = Some(cpu_count);
        }
        if let Some(value) = &self.umask {
            environment.umask = Some(parse_umask(value)?);
        }
        if let Some(network_mode) = &self.network_mode {
            environment.network_mode = network_mode.clone();
        }

        for key in &self.unset_variables {
            environment.environment.remove(key);
        }
        for (key, value) in &self.variables {
            environment.environment.insert(key.clone(), value.clone());
        }
        for key in &self.unset_toolchain {
            environment.toolchain_bindings.remove(key);
        }
        for (key, value) in &self.toolchain {
            environment
                .toolchain_bindings
                .insert(key.clone(), value.clone());
        }
        for (target, variant) in &self.dependency_overrides {
            environment
                .source_file_overrides
                .insert(target.clone(), variant.clone());
        }

        Ok(environment)
    }

    fn validate(&self, project_root: &Path) -> Result<()> {
        if let Some(image) = &self.image {
            if image.trim().is_empty() {
                bail!("environment image cannot be empty");
            }
        }
        if let Some(path) = &self.source_path {
            validate_container_path("source_path", path)?;
        }
        if let Some(path) = &self.build_path {
            validate_container_path("build_path", path)?;
        }
        if let Some(epoch) = self.source_date_epoch {
            if epoch < 0 {
                bail!("source_date_epoch must be non-negative");
            }
        }
        if let Some(epoch) = self.source_mtime_epoch {
            if epoch < 0 {
                bail!("source_mtime_epoch must be non-negative");
            }
        }
        if self.timezone.as_deref().is_some_and(str::is_empty) {
            bail!("timezone cannot be empty");
        }
        if self.locale.as_deref().is_some_and(str::is_empty) {
            bail!("locale cannot be empty");
        }
        if let Some(hostname) = &self.hostname {
            if !valid_hostname(hostname) {
                bail!("hostname contains unsupported characters or length");
            }
        }
        if let Some(cpu_count) = self.cpu_count {
            if !(1..=1024).contains(&cpu_count) {
                bail!("cpu_count must be between 1 and 1024");
            }
        }
        if let Some(value) = &self.umask {
            parse_umask(value)?;
        }
        if let Some(mode) = &self.network_mode {
            if !matches!(mode.as_str(), "default" | "none") {
                bail!("network_mode must be either \"default\" or \"none\"");
            }
        }

        let mut unset_variables = BTreeSet::new();
        for key in &self.unset_variables {
            if !valid_environment_variable_name(key) {
                bail!("unset_variables contains invalid environment variable name {key:?}");
            }
            if !unset_variables.insert(key) {
                bail!("unset_variables contains duplicate key {key:?}");
            }
            if self.variables.contains_key(key) {
                bail!("environment variable {key:?} cannot be both set and unset");
            }
        }
        for key in self.variables.keys() {
            if !valid_environment_variable_name(key) {
                bail!("variables contains invalid environment variable name {key:?}");
            }
        }

        let mut unset_toolchain = BTreeSet::new();
        for key in &self.unset_toolchain {
            validate_toolchain_key(key)?;
            if !unset_toolchain.insert(key) {
                bail!("unset_toolchain contains duplicate key {key:?}");
            }
            if self.toolchain.contains_key(key) {
                bail!("toolchain binding {key:?} cannot be both set and unset");
            }
        }
        for (key, value) in &self.toolchain {
            validate_toolchain_key(key)?;
            if !valid_toolchain_executable(value) {
                bail!("toolchain.{key} contains unsafe executable value {value:?}");
            }
        }

        for (target, variant) in &self.dependency_overrides {
            validate_relative_path("dependency_overrides target", target)?;
            validate_relative_path("dependency_overrides variant", variant)?;
            validate_regular_project_file(project_root, "dependency override target", target)?;
            validate_regular_project_file(project_root, "dependency override variant", variant)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct EnvironmentDelta {
    pub record: EnvironmentDeltaRecord,
    change: EnvironmentChange,
}

#[derive(Debug, Clone)]
enum EnvironmentChange {
    ImageOverride(Option<String>),
    SourcePath(String),
    BuildPath(String),
    SourceCopyOrder(SourceCopyOrder),
    EnvironmentVariable(String, Option<String>),
    ToolchainBinding(String, Option<String>),
    SourceOverride(PathBuf, Option<PathBuf>),
    Hostname(Option<String>),
    CpuCount(Option<u32>),
    SourceMtime(Option<i64>),
    Umask(Option<u32>),
    NetworkMode(String),
}

pub fn diff_environments(
    good: &ControlledEnvironment,
    bad: &ControlledEnvironment,
) -> Vec<EnvironmentDelta> {
    let mut delta = Vec::new();

    if good.image_override != bad.image_override {
        delta.push(EnvironmentDelta {
            record: EnvironmentDeltaRecord {
                variable: "build_image".to_string(),
                baseline_value: good
                    .image_override
                    .clone()
                    .unwrap_or_else(|| "<configured-base-image>".to_string()),
                variant_value: bad
                    .image_override
                    .clone()
                    .unwrap_or_else(|| "<configured-base-image>".to_string()),
            },
            change: EnvironmentChange::ImageOverride(bad.image_override.clone()),
        });
    }
    if good.container_source_path != bad.container_source_path {
        delta.push(EnvironmentDelta {
            record: EnvironmentDeltaRecord {
                variable: "source_path".to_string(),
                baseline_value: good.container_source_path.clone(),
                variant_value: bad.container_source_path.clone(),
            },
            change: EnvironmentChange::SourcePath(bad.container_source_path.clone()),
        });
    }
    if good.container_work_path != bad.container_work_path {
        delta.push(EnvironmentDelta {
            record: EnvironmentDeltaRecord {
                variable: "build_path".to_string(),
                baseline_value: good.container_work_path.clone(),
                variant_value: bad.container_work_path.clone(),
            },
            change: EnvironmentChange::BuildPath(bad.container_work_path.clone()),
        });
    }
    if good.source_copy_order != bad.source_copy_order {
        delta.push(EnvironmentDelta {
            record: EnvironmentDeltaRecord {
                variable: "directory_order".to_string(),
                baseline_value: source_copy_order_name(good.source_copy_order).to_string(),
                variant_value: source_copy_order_name(bad.source_copy_order).to_string(),
            },
            change: EnvironmentChange::SourceCopyOrder(bad.source_copy_order),
        });
    }

    let environment_keys = good
        .environment
        .keys()
        .chain(bad.environment.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for key in environment_keys {
        let left = good.environment.get(&key);
        let right = bad.environment.get(&key);
        if left == right {
            continue;
        }
        delta.push(EnvironmentDelta {
            record: EnvironmentDeltaRecord {
                variable: format!("environment:{key}"),
                baseline_value: left.cloned().unwrap_or_else(|| "<unset>".to_string()),
                variant_value: right.cloned().unwrap_or_else(|| "<unset>".to_string()),
            },
            change: EnvironmentChange::EnvironmentVariable(key.clone(), right.cloned()),
        });
    }

    let toolchain_keys = good
        .toolchain_bindings
        .keys()
        .chain(bad.toolchain_bindings.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for key in toolchain_keys {
        let left = good.toolchain_bindings.get(&key);
        let right = bad.toolchain_bindings.get(&key);
        if left == right {
            continue;
        }
        delta.push(EnvironmentDelta {
            record: EnvironmentDeltaRecord {
                variable: format!("toolchain:{key}"),
                baseline_value: left.cloned().unwrap_or_else(|| "<unset>".to_string()),
                variant_value: right.cloned().unwrap_or_else(|| "<unset>".to_string()),
            },
            change: EnvironmentChange::ToolchainBinding(key.clone(), right.cloned()),
        });
    }

    let override_keys = good
        .source_file_overrides
        .keys()
        .chain(bad.source_file_overrides.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for target in override_keys {
        let left = good.source_file_overrides.get(&target);
        let right = bad.source_file_overrides.get(&target);
        if left == right {
            continue;
        }
        delta.push(EnvironmentDelta {
            record: EnvironmentDeltaRecord {
                variable: format!("dependency:{}", target.display()),
                baseline_value: left
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "<original>".to_string()),
                variant_value: right
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "<original>".to_string()),
            },
            change: EnvironmentChange::SourceOverride(target.clone(), right.cloned()),
        });
    }

    if good.hostname != bad.hostname {
        delta.push(EnvironmentDelta {
            record: EnvironmentDeltaRecord {
                variable: "hostname".to_string(),
                baseline_value: good
                    .hostname
                    .clone()
                    .unwrap_or_else(|| "<runtime-default>".to_string()),
                variant_value: bad
                    .hostname
                    .clone()
                    .unwrap_or_else(|| "<runtime-default>".to_string()),
            },
            change: EnvironmentChange::Hostname(bad.hostname.clone()),
        });
    }
    if good.cpu_count != bad.cpu_count {
        delta.push(EnvironmentDelta {
            record: EnvironmentDeltaRecord {
                variable: "cpu_count".to_string(),
                baseline_value: good
                    .cpu_count
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "<runtime-default>".to_string()),
                variant_value: bad
                    .cpu_count
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "<runtime-default>".to_string()),
            },
            change: EnvironmentChange::CpuCount(bad.cpu_count),
        });
    }
    if good.source_mtime_epoch != bad.source_mtime_epoch {
        delta.push(EnvironmentDelta {
            record: EnvironmentDeltaRecord {
                variable: "source_mtime".to_string(),
                baseline_value: good
                    .source_mtime_epoch
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "<preserved>".to_string()),
                variant_value: bad
                    .source_mtime_epoch
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "<preserved>".to_string()),
            },
            change: EnvironmentChange::SourceMtime(bad.source_mtime_epoch),
        });
    }
    if good.umask != bad.umask {
        delta.push(EnvironmentDelta {
            record: EnvironmentDeltaRecord {
                variable: "umask".to_string(),
                baseline_value: good
                    .umask
                    .map(|value| format!("{value:03o}"))
                    .unwrap_or_else(|| "<runtime-default>".to_string()),
                variant_value: bad
                    .umask
                    .map(|value| format!("{value:03o}"))
                    .unwrap_or_else(|| "<runtime-default>".to_string()),
            },
            change: EnvironmentChange::Umask(bad.umask),
        });
    }
    if good.network_mode != bad.network_mode {
        delta.push(EnvironmentDelta {
            record: EnvironmentDeltaRecord {
                variable: "network_access".to_string(),
                baseline_value: good.network_mode.clone(),
                variant_value: bad.network_mode.clone(),
            },
            change: EnvironmentChange::NetworkMode(bad.network_mode.clone()),
        });
    }

    delta
}

pub fn apply_delta_subset(
    good: &ControlledEnvironment,
    subset: &[EnvironmentDelta],
) -> ControlledEnvironment {
    let mut environment = good.clone();
    for delta in subset {
        match &delta.change {
            EnvironmentChange::ImageOverride(value) => environment.image_override = value.clone(),
            EnvironmentChange::SourcePath(value) => {
                environment.container_source_path = value.clone()
            }
            EnvironmentChange::BuildPath(value) => environment.container_work_path = value.clone(),
            EnvironmentChange::SourceCopyOrder(value) => environment.source_copy_order = *value,
            EnvironmentChange::EnvironmentVariable(key, value) => match value {
                Some(value) => {
                    environment.environment.insert(key.clone(), value.clone());
                }
                None => {
                    environment.environment.remove(key);
                }
            },
            EnvironmentChange::ToolchainBinding(key, value) => match value {
                Some(value) => {
                    environment.toolchain_bindings.insert(key.clone(), value.clone());
                }
                None => {
                    environment.toolchain_bindings.remove(key);
                }
            },
            EnvironmentChange::SourceOverride(target, value) => match value {
                Some(value) => {
                    environment
                        .source_file_overrides
                        .insert(target.clone(), value.clone());
                }
                None => {
                    environment.source_file_overrides.remove(target);
                }
            },
            EnvironmentChange::Hostname(value) => environment.hostname = value.clone(),
            EnvironmentChange::CpuCount(value) => environment.cpu_count = *value,
            EnvironmentChange::SourceMtime(value) => environment.source_mtime_epoch = *value,
            EnvironmentChange::Umask(value) => environment.umask = *value,
            EnvironmentChange::NetworkMode(value) => environment.network_mode = value.clone(),
        }
    }
    environment
}

fn source_copy_order_name(value: SourceCopyOrder) -> &'static str {
    match value {
        SourceCopyOrder::Sorted => "sorted",
        SourceCopyOrder::Reverse => "reverse",
    }
}

fn validate_container_path(field: &str, value: &str) -> Result<()> {
    if !value.starts_with('/') || value.contains(',') || value.contains('\0') {
        bail!("{field} must be an absolute container path without commas or NUL bytes");
    }
    Ok(())
}

fn parse_umask(value: &str) -> Result<u32> {
    let digits = value.strip_prefix("0o").unwrap_or(value);
    if digits.is_empty() || !digits.chars().all(|ch| matches!(ch, '0'..='7')) {
        bail!("umask must be an octal string such as \"022\" or \"0o077\"");
    }
    let parsed = u32::from_str_radix(digits, 8).context("cannot parse umask")?;
    if parsed > 0o777 {
        bail!("umask must be between 000 and 777");
    }
    Ok(parsed)
}

fn valid_hostname(value: &str) -> bool {
    if value.is_empty() || value.len() > 253 {
        return false;
    }
    value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    })
}

fn valid_environment_variable_name(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn validate_toolchain_key(key: &str) -> Result<()> {
    if !matches!(key, "CC" | "CXX" | "LD" | "AR" | "RANLIB" | "RUSTC") {
        bail!("unsupported toolchain binding {key:?}; supported keys: CC, CXX, LD, AR, RANLIB, RUSTC");
    }
    Ok(())
}

fn valid_toolchain_executable(value: &str) -> bool {
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return false;
    }
    let expanded = value
        .replace("{source}", "/src")
        .replace("{build}", "/workspace");
    if expanded.contains('{') || expanded.contains('}') {
        return false;
    }
    expanded.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '+' | '.' | '/' | ':')
    })
}

fn validate_relative_path(field: &str, path: &Path) -> Result<()> {
    if path.is_absolute() {
        bail!("{field} entries must be relative paths: {}", path.display());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("{field} may not escape the project root: {}", path.display());
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::baseline_environment;

    fn config() -> Config {
        toml::from_str(
            r#"
            [build]
            image = "gcc:14"
            command = ["true"]
            outputs = ["out"]
        "#,
        )
        .unwrap()
    }

    #[test]
    fn applies_manifest_on_top_of_canonical_baseline() {
        let config = config();
        let baseline = baseline_environment(&config);
        let manifest: EnvironmentManifest = toml::from_str(
            r#"
            build_path = "/opt/build"
            source_date_epoch = 1234
            timezone = "HST10"
            locale = "C.UTF-8"
            hostname = "known-good"
            cpu_count = 2
            umask = "077"
            network_mode = "none"

            [variables]
            BUILD_FLAVOR = "good"

            [toolchain]
            CC = "gcc"
        "#,
        )
        .unwrap();
        let applied = manifest.apply(&baseline, &config).unwrap();
        assert_eq!(applied.container_work_path, "/opt/build");
        assert_eq!(applied.environment["SOURCE_DATE_EPOCH"], "1234");
        assert_eq!(applied.environment["TZ"], "HST10");
        assert_eq!(applied.environment["LC_ALL"], "C.UTF-8");
        assert_eq!(applied.environment["BUILD_FLAVOR"], "good");
        assert_eq!(applied.hostname.as_deref(), Some("known-good"));
        assert_eq!(applied.cpu_count, Some(2));
        assert_eq!(applied.umask, Some(0o077));
        assert_eq!(applied.network_mode, "none");
        assert_eq!(applied.toolchain_bindings["CC"], "gcc");
    }

    #[test]
    fn delta_subset_changes_only_selected_bad_dimensions() {
        let config = config();
        let good = baseline_environment(&config);
        let mut bad = good.clone();
        bad.container_work_path = "/opt/bad-build".to_string();
        bad.environment.insert("A".to_string(), "bad".to_string());
        bad.environment.insert("B".to_string(), "irrelevant".to_string());

        let delta = diff_environments(&good, &bad);
        assert_eq!(delta.len(), 3);
        let only_a = delta
            .iter()
            .find(|item| item.record.variable == "environment:A")
            .unwrap()
            .clone();
        let candidate = apply_delta_subset(&good, &[only_a]);
        assert_eq!(candidate.environment.get("A").map(String::as_str), Some("bad"));
        assert_eq!(candidate.environment.get("B"), None);
        assert_eq!(candidate.container_work_path, good.container_work_path);
    }

    #[test]
    fn rejects_arbitrary_network_and_parent_dependency_paths() {
        let temp = tempfile::tempdir().unwrap();
        let network: EnvironmentManifest = toml::from_str("network_mode = \"host\"").unwrap();
        assert!(network.validate(temp.path()).is_err());

        let dependency: EnvironmentManifest = toml::from_str(
            r#"
            [dependency_overrides]
            "../Cargo.lock" = "variant.lock"
        "#,
        )
        .unwrap();
        assert!(dependency.validate(temp.path()).is_err());
    }
}
