use serde::{Deserialize, Serialize};

pub const CHECK_REPORT_SCHEMA_MIN: u32 = 5;
pub const CHECK_REPORT_SCHEMA_CURRENT: u32 = 14;
pub const BUILD_EVIDENCE_SCHEMA_MIN: u32 = 1;
pub const BUILD_EVIDENCE_SCHEMA_CURRENT: u32 = 10;
pub const FIX_REPORT_SCHEMA_MIN: u32 = 2;
pub const FIX_REPORT_SCHEMA_CURRENT: u32 = 12;
pub const ENVIRONMENT_COMPARISON_SCHEMA_MIN: u32 = 1;
pub const ENVIRONMENT_COMPARISON_SCHEMA_CURRENT: u32 = 4;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    CheckReport,
    BuildRun,
    BuildFailure,
    FixReport,
    EnvironmentComparisonReport,
}

impl EvidenceKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::CheckReport => "check_report",
            Self::BuildRun => "build_run",
            Self::BuildFailure => "build_failure",
            Self::FixReport => "fix_report",
            Self::EnvironmentComparisonReport => "environment_comparison_report",
        }
    }

    pub const fn minimum_schema_version(self) -> u32 {
        match self {
            Self::CheckReport => CHECK_REPORT_SCHEMA_MIN,
            Self::BuildRun | Self::BuildFailure => BUILD_EVIDENCE_SCHEMA_MIN,
            Self::FixReport => FIX_REPORT_SCHEMA_MIN,
            Self::EnvironmentComparisonReport => ENVIRONMENT_COMPARISON_SCHEMA_MIN,
        }
    }

    pub const fn current_schema_version(self) -> u32 {
        match self {
            Self::CheckReport => CHECK_REPORT_SCHEMA_CURRENT,
            Self::BuildRun | Self::BuildFailure => BUILD_EVIDENCE_SCHEMA_CURRENT,
            Self::FixReport => FIX_REPORT_SCHEMA_CURRENT,
            Self::EnvironmentComparisonReport => ENVIRONMENT_COMPARISON_SCHEMA_CURRENT,
        }
    }
}
