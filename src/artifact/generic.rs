use std::{
    fs::{self, File},
    io::{BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use super::{archive, elf, semantic};
use crate::model::{
    ArtifactComparison, ArtifactRecord, ArtifactType, BuildRun, ControlledEnvironment,
    EmbeddedMarker,
};

pub fn collect_artifacts(
    workspace: &Path,
    outputs: &[PathBuf],
    controlled_environment: &ControlledEnvironment,
) -> Result<Vec<ArtifactRecord>> {
    let markers = marker_candidates(controlled_environment);
    let mut records = Vec::with_capacity(outputs.len());

    for output in outputs {
        let path = workspace.join(output);
        if !path.exists() {
            bail!("declared build output was not produced: {}", output.display());
        }
        if !path.is_file() {
            bail!("declared build output must be a file in v0.1: {}", output.display());
        }

        let mut detected_type = detect_type(&path)?;
        let embedded_markers = scan_markers(&path, &markers)?;
        let (elf_debug_markers, elf_marker_locations, elf_metadata) = if detected_type == ArtifactType::Elf {
            (
                elf::scan_debug_markers(&path, &markers)?,
                elf::scan_debug_marker_locations(&path, &markers)?,
                elf::inspect(&path)?,
            )
        } else {
            (Vec::new(), Vec::new(), None)
        };
        let mut archive_metadata = archive::inspect(&path, &detected_type)?;
        if let Some(metadata) = archive_metadata.as_mut() {
            detected_type = refine_archive_type(&path, &detected_type, metadata);
            metadata.format = archive_format_name(&detected_type, &metadata.format).to_string();
        }
        let semantic_metadata = semantic::inspect(&path, &detected_type, archive_metadata.as_ref())?;

        records.push(ArtifactRecord {
            logical_path: output.clone(),
            sha256: sha256_file(&path)?,
            size_bytes: fs::metadata(&path)?.len(),
            detected_type,
            embedded_markers,
            elf_debug_markers,
            elf_marker_locations,
            elf_metadata,
            archive_metadata,
            semantic_metadata,
        });
    }

    Ok(records)
}

pub fn compare_runs(runs: &[BuildRun]) -> Vec<ArtifactComparison> {
    if runs.is_empty() {
        return Vec::new();
    }

    runs[0]
        .artifacts
        .iter()
        .map(|artifact| {
            let hashes = runs
                .iter()
                .filter_map(|run| {
                    run.artifacts
                        .iter()
                        .find(|candidate| candidate.logical_path == artifact.logical_path)
                })
                .map(|record| record.sha256.clone())
                .collect::<Vec<_>>();
            let equal_across_runs = hashes.len() == runs.len()
                && hashes.windows(2).all(|pair| pair[0] == pair[1]);

            ArtifactComparison {
                logical_path: artifact.logical_path.clone(),
                equal_across_runs,
                hashes,
            }
        })
        .collect()
}

fn sha256_file(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = reader
            .read(&mut buffer)
            .with_context(|| format!("cannot read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

pub(crate) fn detect_type(path: &Path) -> Result<ArtifactType> {
    let mut file = File::open(path)?;
    let mut prefix = [0_u8; 512];
    let read = file.read(&mut prefix)?;
    let bytes = &prefix[..read];

    if bytes.starts_with(b"\x7fELF") {
        return Ok(ArtifactType::Elf);
    }
    if is_pe_coff(&mut file, bytes)? {
        return Ok(ArtifactType::PeCoff);
    }
    if is_macho(bytes) {
        return Ok(ArtifactType::MachO);
    }
    if bytes.starts_with(b"\0asm") {
        return Ok(ArtifactType::Wasm);
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return Ok(match extension_lower(path).as_str() {
            "jar" => ArtifactType::Jar,
            "whl" => ArtifactType::PythonWheel,
            _ => ArtifactType::Zip,
        });
    }
    if bytes.starts_with(b"\x1f\x8b") {
        return Ok(ArtifactType::Gzip);
    }
    if bytes.starts_with(b"!<arch>\n") {
        return Ok(if extension_lower(path) == "deb" {
            ArtifactType::Deb
        } else {
            ArtifactType::Ar
        });
    }
    if is_tar(bytes)
        || path
            .extension()
            .is_some_and(|extension| extension == std::ffi::OsStr::new("tar"))
    {
        return Ok(ArtifactType::Tar);
    }
    Ok(ArtifactType::Generic)
}

fn extension_lower(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_pe_coff(file: &mut File, prefix: &[u8]) -> Result<bool> {
    if prefix.len() < 64 || !prefix.starts_with(b"MZ") {
        return Ok(false);
    }
    let offset = u32::from_le_bytes(
        prefix[0x3c..0x40]
            .try_into()
            .expect("PE offset is inside checked prefix"),
    ) as u64;
    // A wildly large DOS-header offset is not useful evidence and should not
    // make type detection seek across an attacker-controlled sparse file.
    if offset > 16 * 1024 * 1024 {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut signature = [0_u8; 4];
    Ok(file.read_exact(&mut signature).is_ok() && signature == *b"PE\0\0")
}

fn is_macho(bytes: &[u8]) -> bool {
    let Some(magic) = bytes.get(..4) else {
        return false;
    };
    matches!(
        magic,
        b"\xce\xfa\xed\xfe"
            | b"\xfe\xed\xfa\xce"
            | b"\xcf\xfa\xed\xfe"
            | b"\xfe\xed\xfa\xcf"
            | b"\xca\xfe\xba\xbe"
            | b"\xbe\xba\xfe\xca"
            | b"\xca\xfe\xba\xbf"
            | b"\xbf\xba\xfe\xca"
    )
}

fn is_tar(bytes: &[u8]) -> bool {
    bytes.len() >= 512
        && bytes
            .get(257..262)
            .is_some_and(|signature| signature == b"ustar")
}

fn refine_archive_type(
    path: &Path,
    detected_type: &ArtifactType,
    metadata: &crate::model::ArchiveMetadataSummary,
) -> ArtifactType {
    let extension = extension_lower(path);
    let members = metadata
        .members
        .iter()
        .map(|member| member.name.as_str())
        .collect::<Vec<_>>();

    match detected_type {
        ArtifactType::Zip => {
            let wheel_structure = members.iter().any(|name| name.ends_with(".dist-info/WHEEL"))
                && members.iter().any(|name| name.ends_with(".dist-info/METADATA"));
            if extension == "whl" || wheel_structure {
                ArtifactType::PythonWheel
            } else {
                let jar_structure = members.iter().any(|name| *name == "META-INF/MANIFEST.MF")
                    && members.iter().any(|name| name.ends_with(".class"));
                if extension == "jar" || jar_structure {
                    ArtifactType::Jar
                } else {
                    ArtifactType::Zip
                }
            }
        }
        ArtifactType::Ar => {
            let deb_structure = members.iter().any(|name| *name == "debian-binary")
                && members.iter().any(|name| name.starts_with("control.tar"))
                && members.iter().any(|name| name.starts_with("data.tar"));
            if extension == "deb" || deb_structure {
                ArtifactType::Deb
            } else {
                ArtifactType::Ar
            }
        }
        ArtifactType::Tar => {
            let oci_structure = members.iter().any(|name| *name == "oci-layout")
                && members.iter().any(|name| *name == "index.json")
                && members.iter().any(|name| name.starts_with("blobs/sha256/"));
            if oci_structure {
                ArtifactType::OciImage
            } else {
                ArtifactType::Tar
            }
        }
        other => other.clone(),
    }
}

fn archive_format_name<'a>(detected_type: &ArtifactType, fallback: &'a str) -> &'a str {
    match detected_type {
        ArtifactType::Jar => "jar",
        ArtifactType::PythonWheel => "python_wheel",
        ArtifactType::Deb => "deb",
        ArtifactType::OciImage => "oci_image_tar",
        _ => fallback,
    }
}

fn marker_candidates(controlled: &ControlledEnvironment) -> Vec<EmbeddedMarker> {
    let mut markers = Vec::new();
    push_marker(&mut markers, "source_path", &controlled.container_source_path);
    push_marker(&mut markers, "build_path", &controlled.container_work_path);

    if let Some(hostname) = &controlled.hostname {
        push_marker(&mut markers, "hostname", hostname);
    }
    if let Some(cpu_count) = controlled.cpu_count {
        push_marker(&mut markers, "cpu_count", &cpu_count.to_string());
    }

    for (key, value) in &controlled.environment {
        push_marker(&mut markers, key, value);
        if key == "SOURCE_DATE_EPOCH" {
            for human_value in source_date_epoch_markers(value) {
                push_marker(&mut markers, key, human_value);
            }
        }
    }

    markers.sort_by(|left, right| {
        left.variable.cmp(&right.variable).then_with(|| left.value.cmp(&right.value))
    });
    markers.dedup();
    markers
}

fn push_marker(markers: &mut Vec<EmbeddedMarker>, variable: &str, value: &str) {
    if value.len() >= 4 {
        markers.push(EmbeddedMarker { variable: variable.to_string(), value: value.to_string() });
    }
}

fn source_date_epoch_markers(value: &str) -> &'static [&'static str] {
    match value {
        "946684800" => &["Jan  1 2000", "Jan 01 2000", "2000-01-01"],
        "1577836800" => &["Jan  1 2020", "Jan 01 2020", "2020-01-01"],
        _ => &[],
    }
}

fn scan_markers(path: &Path, markers: &[EmbeddedMarker]) -> Result<Vec<EmbeddedMarker>> {
    const MAX_MARKER_SCAN_BYTES: u64 = 64 * 1024 * 1024;
    if fs::metadata(path)?.len() > MAX_MARKER_SCAN_BYTES {
        return Ok(Vec::new());
    }
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(markers
        .iter()
        .filter(|marker| contains_bytes(&bytes, marker.value.as_bytes()))
        .cloned()
        .collect())
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && needle.len() <= haystack.len()
        && haystack.windows(needle.len()).any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_detects_and_scans_generic_file() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("build")).unwrap();
        fs::write(temp.path().join("build/out.txt"), "hello from reprobisect-alt-host").unwrap();
        let controlled = ControlledEnvironment {
            hostname: Some("reprobisect-alt-host".to_string()),
            ..ControlledEnvironment::default()
        };
        let artifacts = collect_artifacts(temp.path(), &[PathBuf::from("build/out.txt")], &controlled).unwrap();
        assert_eq!(artifacts.len(), 1);
        assert!(artifacts[0].embedded_markers.iter().any(|marker| marker.variable == "hostname"));
    }

    fn archive_summary(names: &[&str], format: &str) -> crate::model::ArchiveMetadataSummary {
        crate::model::ArchiveMetadataSummary {
            format: format.to_string(),
            member_count: names.len(),
            members: names
                .iter()
                .enumerate()
                .map(|(index, name)| crate::model::ArchiveMemberMetadata {
                    index,
                    name: (*name).to_string(),
                    size: 0,
                    mtime: None,
                    uid: None,
                    gid: None,
                    mode: None,
                })
                .collect(),
            truncated: false,
            container_mtime: None,
        }
    }

    #[test]
    fn refines_zip_family_artifact_types_structurally() {
        let jar = archive_summary(&["META-INF/MANIFEST.MF", "demo/Main.class"], "zip");
        assert_eq!(
            refine_archive_type(Path::new("artifact.bin"), &ArtifactType::Zip, &jar),
            ArtifactType::Jar
        );

        let wheel = archive_summary(
            &["demo/__init__.py", "demo-1.0.dist-info/WHEEL", "demo-1.0.dist-info/METADATA"],
            "zip",
        );
        assert_eq!(
            refine_archive_type(Path::new("artifact.bin"), &ArtifactType::Zip, &wheel),
            ArtifactType::PythonWheel
        );
    }

    #[test]
    fn refines_deb_and_oci_artifact_types_structurally() {
        let deb = archive_summary(
            &["debian-binary", "control.tar.xz", "data.tar.xz"],
            "ar",
        );
        assert_eq!(
            refine_archive_type(Path::new("artifact.bin"), &ArtifactType::Ar, &deb),
            ArtifactType::Deb
        );

        let oci = archive_summary(
            &["oci-layout", "index.json", "blobs/sha256/abc"],
            "tar",
        );
        assert_eq!(
            refine_archive_type(Path::new("artifact.bin"), &ArtifactType::Tar, &oci),
            ArtifactType::OciImage
        );
    }

    #[test]
    fn detects_pe_macho_wasm_and_extension_typed_packages() {
        let temp = tempfile::tempdir().unwrap();

        let mut pe = vec![0_u8; 128];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c..0x40].copy_from_slice(&(64_u32).to_le_bytes());
        pe[64..68].copy_from_slice(b"PE\0\0");
        let pe_path = temp.path().join("demo.exe");
        fs::write(&pe_path, pe).unwrap();
        assert_eq!(detect_type(&pe_path).unwrap(), ArtifactType::PeCoff);

        let macho_path = temp.path().join("demo.macho");
        fs::write(&macho_path, b"\xcf\xfa\xed\xfe\0\0\0\0").unwrap();
        assert_eq!(detect_type(&macho_path).unwrap(), ArtifactType::MachO);

        let wasm_path = temp.path().join("demo.wasm");
        fs::write(&wasm_path, b"\0asm\x01\0\0\0").unwrap();
        assert_eq!(detect_type(&wasm_path).unwrap(), ArtifactType::Wasm);

        let jar_path = temp.path().join("demo.jar");
        fs::write(&jar_path, b"PK\x03\x04").unwrap();
        assert_eq!(detect_type(&jar_path).unwrap(), ArtifactType::Jar);

        let wheel_path = temp.path().join("demo.whl");
        fs::write(&wheel_path, b"PK\x03\x04").unwrap();
        assert_eq!(detect_type(&wheel_path).unwrap(), ArtifactType::PythonWheel);

        let deb_path = temp.path().join("demo.deb");
        fs::write(&deb_path, b"!<arch>\n").unwrap();
        assert_eq!(detect_type(&deb_path).unwrap(), ArtifactType::Deb);
    }

    #[test]
    fn source_date_epoch_adds_human_readable_marker() {
        let mut controlled = ControlledEnvironment::default();
        controlled.environment.insert("SOURCE_DATE_EPOCH".to_string(), "946684800".to_string());
        let markers = marker_candidates(&controlled);
        assert!(markers.iter().any(|marker| marker.value == "Jan  1 2000"));
    }
}
