mod archive;
mod elf;
mod generic;
mod semantic;

pub use generic::{collect_artifacts, compare_runs};
pub(crate) use archive::compare as compare_archive_metadata;
pub(crate) use semantic::compare as compare_semantic_metadata;
