use std::{collections::{BTreeMap, BTreeSet}, fs, path::Path};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::model::{
    ArchiveMetadataSummary, ArtifactSemanticDifference, ArtifactSemanticSummary, ArtifactType,
};

const MAX_SEMANTIC_ANALYSIS_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EMBEDDED_METADATA_BYTES: usize = 256 * 1024;
const MAX_WASM_SECTIONS: usize = 4096;

pub fn inspect(
    path: &Path,
    detected_type: &ArtifactType,
    archive_metadata: Option<&ArchiveMetadataSummary>,
) -> Result<Option<ArtifactSemanticSummary>> {
    if fs::metadata(path)?.len() > MAX_SEMANTIC_ANALYSIS_BYTES {
        return Ok(None);
    }
    let bytes = fs::read(path)
        .with_context(|| format!("cannot read semantic artifact {}", path.display()))?;
    let summary = match detected_type {
        ArtifactType::Jar => inspect_jar(&bytes, archive_metadata),
        ArtifactType::PythonWheel => inspect_wheel(&bytes, archive_metadata),
        ArtifactType::Deb => inspect_deb(&bytes, archive_metadata),
        ArtifactType::OciImage => inspect_oci(&bytes, archive_metadata),
        ArtifactType::PeCoff => inspect_pe_coff(&bytes),
        ArtifactType::MachO => inspect_macho(&bytes),
        ArtifactType::Wasm => inspect_wasm(&bytes),
        _ => None,
    };
    Ok(summary)
}

pub fn compare(
    baseline: &ArtifactSemanticSummary,
    variant: &ArtifactSemanticSummary,
) -> Vec<ArtifactSemanticDifference> {
    let mut differences = Vec::new();
    if baseline.kind != variant.kind {
        differences.push(ArtifactSemanticDifference {
            field: "kind".to_string(),
            baseline: Some(baseline.kind.clone()),
            variant: Some(variant.kind.clone()),
        });
    }
    if baseline.truncated != variant.truncated {
        differences.push(ArtifactSemanticDifference {
            field: "truncated".to_string(),
            baseline: Some(baseline.truncated.to_string()),
            variant: Some(variant.truncated.to_string()),
        });
    }

    let keys = baseline
        .attributes
        .keys()
        .chain(variant.attributes.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for key in keys {
        let left = baseline.attributes.get(&key).cloned();
        let right = variant.attributes.get(&key).cloned();
        if left != right {
            differences.push(ArtifactSemanticDifference {
                field: key,
                baseline: left,
                variant: right,
            });
        }
    }
    differences
}

fn summary(kind: &str, attributes: BTreeMap<String, String>) -> ArtifactSemanticSummary {
    ArtifactSemanticSummary {
        kind: kind.to_string(),
        attributes,
        truncated: false,
    }
}

fn inspect_jar(
    bytes: &[u8],
    archive_metadata: Option<&ArchiveMetadataSummary>,
) -> Option<ArtifactSemanticSummary> {
    let metadata = archive_metadata?;
    let names = metadata.members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>();
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "manifest_present".into(),
        names.contains(&"META-INF/MANIFEST.MF").to_string(),
    );
    attributes.insert(
        "class_count".into(),
        names.iter().filter(|name| name.ends_with(".class")).count().to_string(),
    );
    attributes.insert(
        "service_descriptor_count".into(),
        names
            .iter()
            .filter(|name| name.starts_with("META-INF/services/") && !name.ends_with('/'))
            .count()
            .to_string(),
    );
    attributes.insert(
        "multi_release_layout".into(),
        names.iter().any(|name| name.starts_with("META-INF/versions/")).to_string(),
    );

    if let Some(content) = zip_stored_member(bytes, "META-INF/MANIFEST.MF") {
        attributes.insert("manifest_content_sha256".into(), sha256_bytes(&content));
        let fields = parse_header_fields(&content);
        if let Some(value) = fields.get("manifest-version") {
            attributes.insert("manifest_version".into(), value.clone());
        }
        attributes.insert(
            "main_class_present".into(),
            fields.contains_key("main-class").to_string(),
        );
        attributes.insert(
            "automatic_module_name_present".into(),
            fields.contains_key("automatic-module-name").to_string(),
        );
        if let Some(value) = fields.get("multi-release") {
            attributes.insert("multi_release_attribute".into(), value.clone());
        }
    }

    Some(summary("jar", attributes))
}

fn inspect_wheel(
    bytes: &[u8],
    archive_metadata: Option<&ArchiveMetadataSummary>,
) -> Option<ArtifactSemanticSummary> {
    let metadata = archive_metadata?;
    let names = metadata.members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>();
    let dist_info_roots = names
        .iter()
        .filter_map(|name| name.split_once(".dist-info/").map(|(prefix, _)| format!("{prefix}.dist-info")))
        .collect::<BTreeSet<_>>();
    let wheel_name = names.iter().find(|name| name.ends_with(".dist-info/WHEEL")).copied();
    let metadata_name = names
        .iter()
        .find(|name| name.ends_with(".dist-info/METADATA"))
        .copied();
    let record_name = names.iter().find(|name| name.ends_with(".dist-info/RECORD")).copied();

    let mut attributes = BTreeMap::new();
    attributes.insert("dist_info_count".into(), dist_info_roots.len().to_string());
    attributes.insert("wheel_metadata_present".into(), wheel_name.is_some().to_string());
    attributes.insert("package_metadata_present".into(), metadata_name.is_some().to_string());
    attributes.insert("record_present".into(), record_name.is_some().to_string());
    attributes.insert(
        "native_extension_count".into(),
        names
            .iter()
            .filter(|name| {
                name.ends_with(".so")
                    || name.ends_with(".pyd")
                    || name.ends_with(".dll")
                    || name.ends_with(".dylib")
            })
            .count()
            .to_string(),
    );

    if let Some(name) = wheel_name {
        if let Some(content) = zip_stored_member(bytes, name) {
            attributes.insert("wheel_content_sha256".into(), sha256_bytes(&content));
            let fields = parse_header_fields(&content);
            if let Some(value) = fields.get("wheel-version") {
                attributes.insert("wheel_version".into(), value.clone());
            }
            if let Some(value) = fields.get("root-is-purelib") {
                attributes.insert("root_is_purelib".into(), value.to_ascii_lowercase());
            }
            let tag_count = String::from_utf8_lossy(&content)
                .lines()
                .filter(|line| line.to_ascii_lowercase().starts_with("tag:"))
                .count();
            attributes.insert("tag_count".into(), tag_count.to_string());
        }
    }
    if let Some(name) = metadata_name {
        if let Some(content) = zip_stored_member(bytes, name) {
            attributes.insert("metadata_content_sha256".into(), sha256_bytes(&content));
            let fields = parse_header_fields(&content);
            if let Some(value) = fields.get("metadata-version") {
                attributes.insert("metadata_version".into(), value.clone());
            }
        }
    }
    if let Some(name) = record_name {
        if let Some(content) = zip_stored_member(bytes, name) {
            attributes.insert("record_content_sha256".into(), sha256_bytes(&content));
        }
    }

    Some(summary("python_wheel", attributes))
}

fn inspect_deb(
    bytes: &[u8],
    archive_metadata: Option<&ArchiveMetadataSummary>,
) -> Option<ArtifactSemanticSummary> {
    let metadata = archive_metadata?;
    let names = metadata.members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>();
    let control = names.iter().find(|name| name.starts_with("control.tar")).copied();
    let data = names.iter().find(|name| name.starts_with("data.tar")).copied();
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "debian_binary_present".into(),
        names.contains(&"debian-binary").to_string(),
    );
    if let Some(name) = control {
        attributes.insert("control_archive".into(), name.to_string());
        attributes.insert("control_compression".into(), archive_suffix(name, "control.tar"));
    }
    if let Some(name) = data {
        attributes.insert("data_archive".into(), name.to_string());
        attributes.insert("data_compression".into(), archive_suffix(name, "data.tar"));
    }
    if let Some(content) = ar_member_content(bytes, "debian-binary") {
        let text = String::from_utf8_lossy(&content);
        let version = text.trim();
        if !version.is_empty() && version.len() <= 32 {
            attributes.insert("debian_binary_version".into(), version.to_string());
        }
    }
    Some(summary("deb", attributes))
}

fn archive_suffix(name: &str, stem: &str) -> String {
    name.strip_prefix(stem)
        .unwrap_or("")
        .trim_start_matches('.')
        .to_string()
}

fn inspect_oci(
    bytes: &[u8],
    archive_metadata: Option<&ArchiveMetadataSummary>,
) -> Option<ArtifactSemanticSummary> {
    let metadata = archive_metadata?;
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "blob_count".into(),
        metadata
            .members
            .iter()
            .filter(|member| member.name.starts_with("blobs/sha256/"))
            .count()
            .to_string(),
    );

    if let Some(content) = tar_member_content(bytes, "oci-layout") {
        attributes.insert("oci_layout_sha256".into(), sha256_bytes(&content));
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&content) {
            if let Some(version) = value.get("imageLayoutVersion").and_then(|v| v.as_str()) {
                attributes.insert("image_layout_version".into(), version.to_string());
            }
        }
    }

    if let Some(content) = tar_member_content(bytes, "index.json") {
        attributes.insert("index_json_sha256".into(), sha256_bytes(&content));
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&content) {
            if let Some(version) = value.get("schemaVersion").and_then(|v| v.as_u64()) {
                attributes.insert("index_schema_version".into(), version.to_string());
            }
            if let Some(manifests) = value.get("manifests").and_then(|v| v.as_array()) {
                attributes.insert("manifest_count".into(), manifests.len().to_string());
                let mut digests = manifests
                    .iter()
                    .filter_map(|manifest| manifest.get("digest").and_then(|v| v.as_str()))
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                digests.sort();
                attributes.insert(
                    "manifest_digest_set_sha256".into(),
                    sha256_bytes(digests.join("\n").as_bytes()),
                );
                let media_types = manifests
                    .iter()
                    .filter_map(|manifest| manifest.get("mediaType").and_then(|v| v.as_str()))
                    .collect::<BTreeSet<_>>();
                attributes.insert("manifest_media_type_count".into(), media_types.len().to_string());
            }
        }
    }

    Some(summary("oci_image", attributes))
}

fn inspect_pe_coff(bytes: &[u8]) -> Option<ArtifactSemanticSummary> {
    if bytes.len() < 64 || !bytes.starts_with(b"MZ") {
        return None;
    }
    let pe_offset = usize::try_from(read_u32_le(bytes, 0x3c)?).ok()?;
    let coff = pe_offset.checked_add(4)?;
    if bytes.get(pe_offset..coff)? != b"PE\0\0" || coff.checked_add(20)? > bytes.len() {
        return None;
    }
    let mut attributes = BTreeMap::new();
    attributes.insert("machine".into(), format!("0x{:04x}", read_u16_le(bytes, coff)?));
    attributes.insert("section_count".into(), read_u16_le(bytes, coff + 2)?.to_string());
    attributes.insert("coff_timestamp".into(), read_u32_le(bytes, coff + 4)?.to_string());
    attributes.insert(
        "optional_header_size".into(),
        read_u16_le(bytes, coff + 16)?.to_string(),
    );
    attributes.insert(
        "characteristics".into(),
        format!("0x{:04x}", read_u16_le(bytes, coff + 18)?),
    );
    Some(summary("pe_coff", attributes))
}

fn inspect_macho(bytes: &[u8]) -> Option<ArtifactSemanticSummary> {
    let magic = bytes.get(..4)?;
    let (little, is_64) = match magic {
        b"\xce\xfa\xed\xfe" => (true, false),
        b"\xcf\xfa\xed\xfe" => (true, true),
        b"\xfe\xed\xfa\xce" => (false, false),
        b"\xfe\xed\xfa\xcf" => (false, true),
        _ => return None,
    };
    let header_size = if is_64 { 32 } else { 28 };
    if bytes.len() < header_size {
        return None;
    }
    let read32 = |offset| read_u32_endian(bytes, offset, little);
    let ncmds = usize::try_from(read32(16)?).ok()?;
    let sizeofcmds = usize::try_from(read32(20)?).ok()?;
    let mut attributes = BTreeMap::new();
    attributes.insert("endianness".into(), if little { "little" } else { "big" }.into());
    attributes.insert("is_64_bit".into(), is_64.to_string());
    attributes.insert("cpu_type".into(), format!("0x{:08x}", read32(4)?));
    attributes.insert("file_type".into(), format!("0x{:08x}", read32(12)?));
    attributes.insert("load_command_count".into(), ncmds.to_string());
    attributes.insert("load_commands_size".into(), sizeofcmds.to_string());

    let commands_end = header_size.checked_add(sizeofcmds)?.min(bytes.len());
    let mut offset = header_size;
    let mut seen = 0_usize;
    while seen < ncmds && offset.checked_add(8)? <= commands_end {
        let cmd = read_u32_endian(bytes, offset, little)?;
        let cmdsize = usize::try_from(read_u32_endian(bytes, offset + 4, little)?).ok()?;
        if cmdsize < 8 || offset.checked_add(cmdsize)? > commands_end {
            break;
        }
        if cmd == 0x1b && cmdsize >= 24 {
            let uuid = bytes.get(offset + 8..offset + 24)?;
            attributes.insert("uuid".into(), hex::encode(uuid));
        }
        offset += cmdsize;
        seen += 1;
    }
    Some(summary("mach_o", attributes))
}

fn inspect_wasm(bytes: &[u8]) -> Option<ArtifactSemanticSummary> {
    if bytes.len() < 8 || !bytes.starts_with(b"\0asm") {
        return None;
    }
    let mut attributes = BTreeMap::new();
    attributes.insert("version".into(), read_u32_le(bytes, 4)?.to_string());
    let mut offset = 8_usize;
    let mut section_count = 0_usize;
    let mut custom_names = Vec::new();
    let mut truncated = false;

    while offset < bytes.len() {
        if section_count >= MAX_WASM_SECTIONS {
            truncated = true;
            break;
        }
        let id = *bytes.get(offset)?;
        offset += 1;
        let (size, used) = read_uleb128(bytes.get(offset..)?)?;
        offset = offset.checked_add(used)?;
        let size = usize::try_from(size).ok()?;
        let end = offset.checked_add(size)?;
        if end > bytes.len() {
            return None;
        }
        if id == 0 {
            let (name_len, name_used) = read_uleb128(bytes.get(offset..end)?)?;
            let name_start = offset.checked_add(name_used)?;
            let name_end = name_start.checked_add(usize::try_from(name_len).ok()?)?;
            if name_end <= end {
                let name = String::from_utf8_lossy(bytes.get(name_start..name_end)?).to_string();
                custom_names.push(name);
            }
        }
        section_count += 1;
        offset = end;
    }

    attributes.insert("section_count".into(), section_count.to_string());
    attributes.insert("custom_section_count".into(), custom_names.len().to_string());
    attributes.insert(
        "name_section_present".into(),
        custom_names.iter().any(|name| name == "name").to_string(),
    );
    attributes.insert(
        "producers_section_present".into(),
        custom_names.iter().any(|name| name == "producers").to_string(),
    );
    attributes.insert(
        "custom_section_order_sha256".into(),
        sha256_bytes(custom_names.join("\n").as_bytes()),
    );

    Some(ArtifactSemanticSummary {
        kind: "wasm".to_string(),
        attributes,
        truncated,
    })
}

fn parse_header_fields(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for line in String::from_utf8_lossy(bytes).lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let key = name.trim().to_ascii_lowercase();
        if key.is_empty() {
            continue;
        }
        fields.entry(key).or_insert_with(|| value.trim().to_string());
    }
    fields
}

fn zip_stored_member(bytes: &[u8], target: &str) -> Option<Vec<u8>> {
    let mut search = 0_usize;
    while search.checked_add(46)? <= bytes.len() {
        let relative = bytes.get(search..)?.windows(4).position(|w| w == b"PK\x01\x02")?;
        let offset = search.checked_add(relative)?;
        let header = bytes.get(offset..offset + 46)?;
        let flags = read_u16_le(header, 8)?;
        let method = read_u16_le(header, 10)?;
        let compressed_size = usize::try_from(read_u32_le(header, 20)?).ok()?;
        let name_len = usize::from(read_u16_le(header, 28)?);
        let extra_len = usize::from(read_u16_le(header, 30)?);
        let comment_len = usize::from(read_u16_le(header, 32)?);
        let local_offset = usize::try_from(read_u32_le(header, 42)?).ok()?;
        let name_start = offset.checked_add(46)?;
        let name_end = name_start.checked_add(name_len)?;
        let record_end = name_end.checked_add(extra_len)?.checked_add(comment_len)?;
        if record_end > bytes.len() {
            return None;
        }
        let name = String::from_utf8_lossy(bytes.get(name_start..name_end)?);
        if name == target {
            if method != 0 || flags & 0x1 != 0 || compressed_size > MAX_EMBEDDED_METADATA_BYTES {
                return None;
            }
            let local = bytes.get(local_offset..local_offset.checked_add(30)?)?;
            if local.get(..4)? != b"PK\x03\x04" {
                return None;
            }
            let local_name_len = usize::from(read_u16_le(local, 26)?);
            let local_extra_len = usize::from(read_u16_le(local, 28)?);
            let data_start = local_offset
                .checked_add(30)?
                .checked_add(local_name_len)?
                .checked_add(local_extra_len)?;
            let data_end = data_start.checked_add(compressed_size)?;
            return Some(bytes.get(data_start..data_end)?.to_vec());
        }
        search = record_end;
    }
    None
}

fn ar_member_content(bytes: &[u8], target: &str) -> Option<Vec<u8>> {
    if !bytes.starts_with(b"!<arch>\n") {
        return None;
    }
    let mut offset = 8_usize;
    while offset.checked_add(60)? <= bytes.len() {
        let header = bytes.get(offset..offset + 60)?;
        if header.get(58..60)? != b"`\n" {
            return None;
        }
        let raw_name = String::from_utf8_lossy(header.get(0..16)?)
            .trim()
            .trim_end_matches('/')
            .to_string();
        let size = String::from_utf8_lossy(header.get(48..58)?)
            .trim()
            .parse::<usize>()
            .ok()?;
        let data_start = offset.checked_add(60)?;
        let data_end = data_start.checked_add(size)?;
        if data_end > bytes.len() {
            return None;
        }
        if raw_name == target {
            if size > MAX_EMBEDDED_METADATA_BYTES {
                return None;
            }
            return Some(bytes.get(data_start..data_end)?.to_vec());
        }
        offset = data_end.checked_add(size % 2)?;
    }
    None
}

fn tar_member_content(bytes: &[u8], target: &str) -> Option<Vec<u8>> {
    let mut offset = 0_usize;
    while offset.checked_add(512)? <= bytes.len() {
        let header = bytes.get(offset..offset + 512)?;
        if header.iter().all(|byte| *byte == 0) {
            return None;
        }
        let name = tar_name(header);
        let size = parse_tar_octal(header.get(124..136)?)?;
        let size = usize::try_from(size).ok()?;
        let data_start = offset.checked_add(512)?;
        let data_end = data_start.checked_add(size)?;
        if data_end > bytes.len() {
            return None;
        }
        if name == target {
            if size > MAX_EMBEDDED_METADATA_BYTES {
                return None;
            }
            return Some(bytes.get(data_start..data_end)?.to_vec());
        }
        let padded = size.checked_add(511)? / 512 * 512;
        offset = data_start.checked_add(padded)?;
    }
    None
}

fn tar_name(header: &[u8]) -> String {
    let name = nul_string(header.get(0..100).unwrap_or_default());
    let prefix = nul_string(header.get(345..500).unwrap_or_default());
    if prefix.is_empty() {
        name
    } else if name.is_empty() {
        prefix
    } else {
        format!("{prefix}/{name}")
    }
}

fn nul_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

fn parse_tar_octal(bytes: &[u8]) -> Option<u64> {
    let text = String::from_utf8_lossy(bytes)
        .trim_matches(|ch: char| ch == '\0' || ch.is_ascii_whitespace())
        .to_string();
    let trimmed = text.trim_start_matches('0');
    if trimmed.is_empty() {
        Some(0)
    } else {
        u64::from_str_radix(trimmed, 8).ok()
    }
}

fn read_uleb128(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0_u64;
    let mut shift = 0_u32;
    for (index, byte) in bytes.iter().copied().enumerate().take(10) {
        value |= u64::from(byte & 0x7f).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
        shift += 7;
    }
    None
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?))
}

fn read_u32_endian(bytes: &[u8], offset: usize, little: bool) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(if little { u32::from_le_bytes(raw) } else { u32::from_be_bytes(raw) })
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_compare_reports_attribute_change() {
        let mut left = BTreeMap::new();
        left.insert("coff_timestamp".to_string(), "1".to_string());
        let mut right = BTreeMap::new();
        right.insert("coff_timestamp".to_string(), "2".to_string());
        let differences = compare(&summary("pe_coff", left), &summary("pe_coff", right));
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].field, "coff_timestamp");
    }

    #[test]
    fn parses_minimal_pe_timestamp() {
        let mut bytes = vec![0_u8; 128];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&64_u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[72..76].copy_from_slice(&1234_u32.to_le_bytes());
        let summary = inspect_pe_coff(&bytes).unwrap();
        assert_eq!(summary.attributes.get("coff_timestamp"), Some(&"1234".to_string()));
    }

    #[test]
    fn parses_minimal_wasm_version() {
        let summary = inspect_wasm(b"\0asm\x01\0\0\0").unwrap();
        assert_eq!(summary.attributes.get("version"), Some(&"1".to_string()));
        assert_eq!(summary.attributes.get("section_count"), Some(&"0".to_string()));
    }
}
