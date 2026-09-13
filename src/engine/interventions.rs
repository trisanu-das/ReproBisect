use std::collections::BTreeMap;

use crate::{
    config::Config,
    model::{ControlledEnvironment, Intervention, InterventionKind, SourceCopyOrder},
};

pub const BASELINE_SOURCE_DATE_EPOCH: &str = "946684800";
pub const VARIANT_SOURCE_DATE_EPOCH: &str = "1577836800";
pub const BASELINE_SOURCE_MTIME_EPOCH: i64 = 946_684_800;
pub const VARIANT_SOURCE_MTIME_EPOCH: i64 = 1_577_836_800;
pub const BASELINE_UMASK: u32 = 0o022;
pub const VARIANT_UMASK: u32 = 0o077;

#[derive(Debug, Clone)]
pub struct PlannedIntervention {
    pub intervention: Intervention,
    pub environment: ControlledEnvironment,
}

pub fn baseline_environment(config: &Config) -> ControlledEnvironment {
    let mut environment = BTreeMap::new();
    environment.insert("SOURCE_DATE_EPOCH".to_string(), BASELINE_SOURCE_DATE_EPOCH.to_string());
    environment.insert("TZ".to_string(), "UTC".to_string());
    environment.insert("LANG".to_string(), "C".to_string());
    environment.insert("LC_ALL".to_string(), "C".to_string());

    for (key, values) in &config.experiments.environment_variables {
        environment.insert(key.clone(), values[0].clone());
    }

    let toolchain_bindings = config
        .experiments
        .toolchain_variables
        .iter()
        .map(|(key, values)| (key.clone(), values[0].clone()))
        .collect();
    // Rewrite dependency targets in every run, including the baseline, so the
    // intervention does not differ merely because one side performed a write.
    // The runner restores target mode/atime/mtime after rewriting.
    let source_file_overrides = config
        .experiments
        .dependency_variants
        .iter()
        .map(|variant| (variant.target.clone(), variant.target.clone()))
        .collect();

    ControlledEnvironment {
        image_override: None,
        container_source_path: "/src".to_string(),
        container_work_path: "/workspace".to_string(),
        source_copy_order: SourceCopyOrder::Sorted,
        environment,
        toolchain_bindings,
        source_file_overrides,
        hostname: Some("reprobisect-control".to_string()),
        cpu_count: config.experiments.dimensions.cpu_count.then_some(1),
        source_mtime_epoch: config
            .experiments
            .dimensions
            .source_mtime
            .then_some(BASELINE_SOURCE_MTIME_EPOCH),
        umask: config.experiments.dimensions.umask.then_some(BASELINE_UMASK),
        network_mode: "default".to_string(),
        network_trace: config.experiments.network_trace,
        file_input_trace: config.experiments.file_input_trace,
        syscall_trace_max_bytes: config.experiments.syscall_trace_max_bytes,
        runtime_dependency_provenance: config.experiments.runtime_dependency_provenance,
        dependency_cache_paths: config.experiments.dependency_cache_paths.clone(),
        dependency_cache_max_files: config.experiments.dependency_cache_max_files,
        dependency_cache_max_bytes: config.experiments.dependency_cache_max_bytes,
    }
}

pub fn plan_interventions(
    config: &Config,
    baseline: &ControlledEnvironment,
) -> Vec<PlannedIntervention> {
    let mut planned = Vec::new();
    let dimensions = &config.experiments.dimensions;

    for (index, image) in config.experiments.image_variants.iter().enumerate() {
        let mut environment = baseline.clone();
        environment.image_override = Some(image.clone());
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: format!("build-image-{}", index + 1),
                kind: InterventionKind::BuildImage,
                variable: "build_image".to_string(),
                baseline_value: config.build.image.clone(),
                variant_value: image.clone(),
                description: "rebuild the identical source snapshot in an alternative container/toolchain image".to_string(),
            },
            environment,
        });
    }

    for (key, values) in &config.experiments.toolchain_variables {
        let mut environment = baseline.clone();
        environment
            .toolchain_bindings
            .insert(key.clone(), values[1].clone());
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: format!("toolchain-{key}"),
                kind: InterventionKind::ToolchainExecutable,
                variable: format!("toolchain:{key}"),
                baseline_value: values[0].clone(),
                variant_value: values[1].clone(),
                description: format!(
                    "change only the {key} executable binding inside the same resolved build image"
                ),
            },
            environment,
        });
    }

    for variant in &config.experiments.dependency_variants {
        let mut environment = baseline.clone();
        environment
            .source_file_overrides
            .insert(variant.target.clone(), variant.variant_file.clone());
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: format!("dependency-{}", variant.id),
                kind: InterventionKind::DependencyFile,
                variable: format!("dependency:{}", variant.target.display()),
                baseline_value: variant.target.display().to_string(),
                variant_value: variant.variant_file.display().to_string(),
                description: format!(
                    "replace dependency declaration {} with controlled variant {} in the fresh workspace",
                    variant.target.display(),
                    variant.variant_file.display()
                ),
            },
            environment,
        });
    }

    if dimensions.network_access {
        let mut environment = baseline.clone();
        environment.network_mode = "none".to_string();
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: "network-none".to_string(),
                kind: InterventionKind::NetworkAccess,
                variable: "network_access".to_string(),
                baseline_value: "default".to_string(),
                variant_value: "none".to_string(),
                description: "disable container networking to test whether the build or artifact depends on network availability".to_string(),
            },
            environment,
        });
    }

    if dimensions.source_path {
        let mut environment = baseline.clone();
        environment.container_source_path = "/opt/reprobisect/source".to_string();
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: "source-path".to_string(),
                kind: InterventionKind::SourcePath,
                variable: "source_path".to_string(),
                baseline_value: baseline.container_source_path.clone(),
                variant_value: environment.container_source_path.clone(),
                description: "expose the same source snapshot at a different in-container source path".to_string(),
            },
            environment,
        });
    }

    if dimensions.build_path {
        let mut environment = baseline.clone();
        environment.container_work_path = "/opt/reprobisect/build".to_string();
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: "build-path".to_string(),
                kind: InterventionKind::BuildPath,
                variable: "build_path".to_string(),
                baseline_value: baseline.container_work_path.clone(),
                variant_value: environment.container_work_path.clone(),
                description: "rebuild from a different in-container workspace path".to_string(),
            },
            environment,
        });
    }

    if dimensions.source_date_epoch {
        let mut environment = baseline.clone();
        environment.environment.insert(
            "SOURCE_DATE_EPOCH".to_string(),
            VARIANT_SOURCE_DATE_EPOCH.to_string(),
        );
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: "source-date-epoch".to_string(),
                kind: InterventionKind::SourceDateEpoch,
                variable: "SOURCE_DATE_EPOCH".to_string(),
                baseline_value: BASELINE_SOURCE_DATE_EPOCH.to_string(),
                variant_value: VARIANT_SOURCE_DATE_EPOCH.to_string(),
                description: "change the standardized build timestamp input".to_string(),
            },
            environment,
        });
    }

    if dimensions.source_mtime {
        let mut environment = baseline.clone();
        environment.source_mtime_epoch = Some(VARIANT_SOURCE_MTIME_EPOCH);
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: "source-mtime".to_string(),
                kind: InterventionKind::SourceMtime,
                variable: "source_mtime".to_string(),
                baseline_value: BASELINE_SOURCE_MTIME_EPOCH.to_string(),
                variant_value: VARIANT_SOURCE_MTIME_EPOCH.to_string(),
                description: "change copied source-tree mtimes while keeping source bytes fixed".to_string(),
            },
            environment,
        });
    }

    if dimensions.timezone {
        let mut environment = baseline.clone();
        environment.environment.insert("TZ".to_string(), "HST10".to_string());
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: "timezone".to_string(),
                kind: InterventionKind::Timezone,
                variable: "TZ".to_string(),
                baseline_value: "UTC".to_string(),
                variant_value: "HST10".to_string(),
                description: "change process timezone while keeping source and toolchain fixed".to_string(),
            },
            environment,
        });
    }

    if dimensions.locale {
        let mut environment = baseline.clone();
        environment.environment.insert("LANG".to_string(), "C.UTF-8".to_string());
        environment.environment.insert("LC_ALL".to_string(), "C.UTF-8".to_string());
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: "locale".to_string(),
                kind: InterventionKind::Locale,
                variable: "locale".to_string(),
                baseline_value: "C".to_string(),
                variant_value: "C.UTF-8".to_string(),
                description: "change process locale while keeping other controlled inputs fixed".to_string(),
            },
            environment,
        });
    }

    if dimensions.hostname {
        let mut environment = baseline.clone();
        environment.hostname = Some("reprobisect-variant".to_string());
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: "hostname".to_string(),
                kind: InterventionKind::Hostname,
                variable: "hostname".to_string(),
                baseline_value: baseline.hostname.clone().unwrap_or_else(|| "<default>".to_string()),
                variant_value: "reprobisect-variant".to_string(),
                description: "change the container hostname".to_string(),
            },
            environment,
        });
    }

    if dimensions.cpu_count {
        let mut environment = baseline.clone();
        environment.cpu_count = Some(2);
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: "cpu-count".to_string(),
                kind: InterventionKind::CpuCount,
                variable: "cpu_count".to_string(),
                baseline_value: "1".to_string(),
                variant_value: "2".to_string(),
                description: "change the container CPU quota and REPROBISECT_CPU_COUNT hint".to_string(),
            },
            environment,
        });
    }

    if dimensions.umask {
        let mut environment = baseline.clone();
        environment.umask = Some(VARIANT_UMASK);
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: "umask".to_string(),
                kind: InterventionKind::Umask,
                variable: "umask".to_string(),
                baseline_value: format!("{BASELINE_UMASK:03o}"),
                variant_value: format!("{VARIANT_UMASK:03o}"),
                description: "change the process file-creation mask".to_string(),
            },
            environment,
        });
    }

    if dimensions.directory_order {
        let mut environment = baseline.clone();
        environment.source_copy_order = SourceCopyOrder::Reverse;
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: "directory-order".to_string(),
                kind: InterventionKind::DirectoryOrder,
                variable: "directory_order".to_string(),
                baseline_value: "sorted".to_string(),
                variant_value: "reverse".to_string(),
                description: "materialize the fresh source snapshot in reverse lexical creation order as a best-effort filesystem enumeration perturbation".to_string(),
            },
            environment,
        });
    }

    for (key, values) in &config.experiments.environment_variables {
        let mut environment = baseline.clone();
        environment.environment.insert(key.clone(), values[1].clone());
        planned.push(PlannedIntervention {
            intervention: Intervention {
                id: format!("env-{key}"),
                kind: InterventionKind::EnvironmentVariable,
                variable: key.clone(),
                baseline_value: values[0].clone(),
                variant_value: values[1].clone(),
                description: format!("change configured environment variable {key}"),
            },
            environment,
        });
    }

    planned
}

pub fn combine_interventions(
    baseline: &ControlledEnvironment,
    interventions: &[PlannedIntervention],
) -> ControlledEnvironment {
    let mut combined = baseline.clone();
    for planned in interventions {
        if planned.environment.image_override != baseline.image_override {
            combined.image_override = planned.environment.image_override.clone();
        }
        if planned.environment.container_source_path != baseline.container_source_path {
            combined.container_source_path = planned.environment.container_source_path.clone();
        }
        if planned.environment.container_work_path != baseline.container_work_path {
            combined.container_work_path = planned.environment.container_work_path.clone();
        }
        if planned.environment.source_copy_order != baseline.source_copy_order {
            combined.source_copy_order = planned.environment.source_copy_order;
        }
        for (key, value) in &planned.environment.toolchain_bindings {
            if baseline.toolchain_bindings.get(key) != Some(value) {
                combined.toolchain_bindings.insert(key.clone(), value.clone());
            }
        }
        for (target, variant_file) in &planned.environment.source_file_overrides {
            if baseline.source_file_overrides.get(target) != Some(variant_file) {
                combined
                    .source_file_overrides
                    .insert(target.clone(), variant_file.clone());
            }
        }
        if planned.environment.hostname != baseline.hostname {
            combined.hostname = planned.environment.hostname.clone();
        }
        if planned.environment.cpu_count != baseline.cpu_count {
            combined.cpu_count = planned.environment.cpu_count;
        }
        if planned.environment.source_mtime_epoch != baseline.source_mtime_epoch {
            combined.source_mtime_epoch = planned.environment.source_mtime_epoch;
        }
        if planned.environment.umask != baseline.umask {
            combined.umask = planned.environment.umask;
        }
        if planned.environment.network_mode != baseline.network_mode {
            combined.network_mode = planned.environment.network_mode.clone();
        }
        if planned.environment.network_trace != baseline.network_trace {
            combined.network_trace = planned.environment.network_trace;
        }
        if planned.environment.file_input_trace != baseline.file_input_trace {
            combined.file_input_trace = planned.environment.file_input_trace;
        }
        if planned.environment.syscall_trace_max_bytes != baseline.syscall_trace_max_bytes {
            combined.syscall_trace_max_bytes = planned.environment.syscall_trace_max_bytes;
        }
        if planned.environment.runtime_dependency_provenance != baseline.runtime_dependency_provenance {
            combined.runtime_dependency_provenance = planned.environment.runtime_dependency_provenance;
        }
        if planned.environment.dependency_cache_paths != baseline.dependency_cache_paths {
            combined.dependency_cache_paths = planned.environment.dependency_cache_paths.clone();
        }
        if planned.environment.dependency_cache_max_files != baseline.dependency_cache_max_files {
            combined.dependency_cache_max_files = planned.environment.dependency_cache_max_files;
        }
        if planned.environment.dependency_cache_max_bytes != baseline.dependency_cache_max_bytes {
            combined.dependency_cache_max_bytes = planned.environment.dependency_cache_max_bytes;
        }
        for (key, value) in &planned.environment.environment {
            if baseline.environment.get(key) != Some(value) {
                combined.environment.insert(key.clone(), value.clone());
            }
        }
    }
    combined
}

pub fn eligible_for_interaction_search(intervention: &PlannedIntervention) -> bool {
    !matches!(
        intervention.intervention.kind,
        InterventionKind::CpuCount
            | InterventionKind::BuildImage
            | InterventionKind::ToolchainExecutable
            | InterventionKind::DependencyFile
            | InterventionKind::NetworkAccess
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_pins_core_environment() {
        let config: Config = toml::from_str(r#"
            [build]
            image = "gcc:14"
            command = ["true"]
            outputs = ["out"]
        "#).unwrap();
        let baseline = baseline_environment(&config);
        assert_eq!(baseline.environment["TZ"], "UTC");
        assert_eq!(baseline.environment["LC_ALL"], "C");
        assert_eq!(baseline.environment["SOURCE_DATE_EPOCH"], BASELINE_SOURCE_DATE_EPOCH);
        assert_eq!(baseline.image_override, None);
        assert_eq!(baseline.container_source_path, "/src");
        assert_eq!(baseline.container_work_path, "/workspace");
        assert_eq!(baseline.source_copy_order, SourceCopyOrder::Sorted);
        assert!(baseline.toolchain_bindings.is_empty());
        assert!(baseline.source_file_overrides.is_empty());
        assert!(!baseline.network_trace);
        assert_eq!(baseline.hostname.as_deref(), Some("reprobisect-control"));
        assert_eq!(baseline.source_mtime_epoch, None);
        assert_eq!(baseline.umask, None);
    }

    #[test]
    fn plans_image_and_network_interventions() {
        let config: Config = toml::from_str(r#"
            [build]
            image = "gcc:14"
            command = ["true"]
            outputs = ["out"]

            [experiments]
            image_variants = ["gcc:15"]

            [experiments.dimensions]
            network_access = true
            build_path = false
            source_date_epoch = false
            timezone = false
            locale = false
            hostname = false
        "#).unwrap();
        let baseline = baseline_environment(&config);
        let planned = plan_interventions(&config, &baseline);
        let image = planned.iter().find(|p| p.intervention.kind == InterventionKind::BuildImage).unwrap();
        assert_eq!(image.environment.image_override.as_deref(), Some("gcc:15"));
        let network = planned.iter().find(|p| p.intervention.kind == InterventionKind::NetworkAccess).unwrap();
        assert_eq!(network.environment.network_mode, "none");
        assert!(!eligible_for_interaction_search(image));
        assert!(!eligible_for_interaction_search(network));
    }

    #[test]
    fn plans_narrow_toolchain_and_dependency_file_interventions() {
        let config: Config = toml::from_str(r#"
            [build]
            image = "gcc:14"
            command = ["true"]
            outputs = ["out"]

            [experiments.toolchain_variables]
            CC = ["gcc", "clang"]

            [[experiments.dependency_variants]]
            id = "requirements"
            target = "requirements.txt"
            variant_file = "requirements.variant.txt"
        "#).unwrap();
        let baseline = baseline_environment(&config);
        assert_eq!(baseline.toolchain_bindings["CC"], "gcc");
        let planned = plan_interventions(&config, &baseline);
        let toolchain = planned
            .iter()
            .find(|item| item.intervention.kind == InterventionKind::ToolchainExecutable)
            .unwrap();
        assert_eq!(toolchain.environment.toolchain_bindings["CC"], "clang");
        assert!(!eligible_for_interaction_search(toolchain));

        let dependency = planned
            .iter()
            .find(|item| item.intervention.kind == InterventionKind::DependencyFile)
            .unwrap();
        assert_eq!(
            dependency
                .environment
                .source_file_overrides
                .get(&std::path::PathBuf::from("requirements.txt")),
            Some(&std::path::PathBuf::from("requirements.variant.txt"))
        );
        assert!(!eligible_for_interaction_search(dependency));
    }

    #[test]
    fn source_and_build_path_interventions_are_independent() {
        let config: Config = toml::from_str(r#"
            [build]
            image = "gcc:14"
            command = ["true"]
            outputs = ["out"]

            [experiments.dimensions]
            source_path = true
            build_path = true
            source_date_epoch = false
            timezone = false
            locale = false
            hostname = false
        "#).unwrap();
        let baseline = baseline_environment(&config);
        let planned = plan_interventions(&config, &baseline);

        let source_path = planned
            .iter()
            .find(|planned| planned.intervention.kind == InterventionKind::SourcePath)
            .unwrap();
        assert_ne!(source_path.environment.container_source_path, baseline.container_source_path);
        assert_eq!(source_path.environment.container_work_path, baseline.container_work_path);

        let build_path = planned
            .iter()
            .find(|planned| planned.intervention.kind == InterventionKind::BuildPath)
            .unwrap();
        assert_eq!(build_path.environment.container_source_path, baseline.container_source_path);
        assert_ne!(build_path.environment.container_work_path, baseline.container_work_path);

        let combined = combine_interventions(&baseline, &[source_path.clone(), build_path.clone()]);
        assert_eq!(combined.container_source_path, source_path.environment.container_source_path);
        assert_eq!(combined.container_work_path, build_path.environment.container_work_path);
    }

    #[test]
    fn source_mtime_and_umask_baselines_are_opt_in() {
        let config: Config = toml::from_str(r#"
            [build]
            image = "gcc:14"
            command = ["true"]
            outputs = ["out"]

            [experiments.dimensions]
            source_mtime = true
            umask = true
        "#).unwrap();
        let baseline = baseline_environment(&config);
        assert_eq!(baseline.source_mtime_epoch, Some(BASELINE_SOURCE_MTIME_EPOCH));
        assert_eq!(baseline.umask, Some(BASELINE_UMASK));
    }

    #[test]
    fn combines_independent_interventions_without_reverting_each_other() {
        let config: Config = toml::from_str(r#"
            [build]
            image = "gcc:14"
            command = ["true"]
            outputs = ["out"]

            [experiments.dimensions]
            build_path = false
            source_date_epoch = false
            timezone = true
            locale = false
            hostname = true
            source_mtime = false
            cpu_count = false
            umask = false
        "#).unwrap();
        let baseline = baseline_environment(&config);
        let planned = plan_interventions(&config, &baseline);
        let combined = combine_interventions(&baseline, &planned);
        assert_eq!(combined.environment["TZ"], "HST10");
        assert_eq!(combined.hostname.as_deref(), Some("reprobisect-variant"));
    }
}
