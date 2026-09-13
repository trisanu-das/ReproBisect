use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result};

use crate::model::{
    ArchiveMemberMetadata, ArchiveMetadataDifference, ArchiveMetadataField,
    ArchiveMetadataSummary, ArtifactType,
};

const MAX_ARCHIVE_ANALYSIS_BYTES: u64 = 128 * 1024 * 1024;
const MAX_RECORDED_MEMBERS: usize = 512;

pub fn inspect(
    path: &Path,
    detected_type: &ArtifactType,
) -> Result<Option<ArchiveMetadataSummary>> {
    if fs::metadata(path)?.len() > MAX_ARCHIVE_ANALYSIS_BYTES {
        return Ok(None);
    }

    let bytes = fs::read(path).with_context(|| format!("cannot read archive {}", path.display()))?;
    let summary = match detected_type {
        ArtifactType::Ar | ArtifactType::Deb => inspect_ar(&bytes),
        ArtifactType::Tar | ArtifactType::OciImage => inspect_tar(&bytes),
        ArtifactType::Zip | ArtifactType::Jar | ArtifactType::PythonWheel => inspect_zip(&bytes),
        ArtifactType::Gzip => inspect_gzip(&bytes),
        _ => None,
    };
    Ok(summary)
}

pub fn compare(
    baseline: &ArchiveMetadataSummary,
    variant: &ArchiveMetadataSummary,
) -> Vec<ArchiveMetadataDifference> {
    let mut differences = Vec::new();

    if baseline.member_count != variant.member_count {
        differences.push(ArchiveMetadataDifference {
            field: ArchiveMetadataField::MemberCount,
            member: None,
            baseline: Some(baseline.member_count.to_string()),
            variant: Some(variant.member_count.to_string()),
        });
    }

    if baseline.container_mtime != variant.container_mtime {
        differences.push(ArchiveMetadataDifference {
            field: ArchiveMetadataField::ContainerMtime,
            member: None,
            baseline: baseline.container_mtime.map(|value| value.to_string()),
            variant: variant.container_mtime.map(|value| value.to_string()),
        });
    }

    let baseline_order = baseline
        .members
        .iter()
        .map(|member| member.name.as_str())
        .collect::<Vec<_>>();
    let variant_order = variant
        .members
        .iter()
        .map(|member| member.name.as_str())
        .collect::<Vec<_>>();
    if baseline_order != variant_order {
        differences.push(ArchiveMetadataDifference {
            field: ArchiveMetadataField::MemberOrder,
            member: None,
            baseline: Some(baseline_order.join(", ")),
            variant: Some(variant_order.join(", ")),
        });
    }

    match (
        unique_members_by_name(&baseline.members),
        unique_members_by_name(&variant.members),
    ) {
        (Some(left), Some(right)) => {
            let names = left
                .keys()
                .copied()
                .chain(right.keys().copied())
                .collect::<BTreeSet<_>>();
            for name in names {
                if let (Some(baseline_member), Some(variant_member)) =
                    (left.get(name), right.get(name))
                {
                    compare_member(baseline_member, variant_member, &mut differences);
                }
            }
        }
        _ => {
            for (baseline_member, variant_member) in
                baseline.members.iter().zip(&variant.members)
            {
                if baseline_member.name == variant_member.name {
                    compare_member(baseline_member, variant_member, &mut differences);
                }
            }
        }
    }

    differences.sort_by(|left, right| {
        left.field
            .cmp(&right.field)
            .then_with(|| left.member.cmp(&right.member))
            .then_with(|| left.baseline.cmp(&right.baseline))
            .then_with(|| left.variant.cmp(&right.variant))
    });
    differences.dedup();
    differences
}

fn unique_members_by_name<'a>(
    members: &'a [ArchiveMemberMetadata],
) -> Option<BTreeMap<&'a str, &'a ArchiveMemberMetadata>> {
    let mut result = BTreeMap::new();
    for member in members {
        if result.insert(member.name.as_str(), member).is_some() {
            return None;
        }
    }
    Some(result)
}

fn compare_member(
    baseline: &ArchiveMemberMetadata,
    variant: &ArchiveMemberMetadata,
    differences: &mut Vec<ArchiveMetadataDifference>,
) {
    compare_value(
        ArchiveMetadataField::Mtime,
        &baseline.name,
        baseline.mtime.map(i128::from),
        variant.mtime.map(i128::from),
        differences,
    );
    compare_value(
        ArchiveMetadataField::Uid,
        &baseline.name,
        baseline.uid.map(i128::from),
        variant.uid.map(i128::from),
        differences,
    );
    compare_value(
        ArchiveMetadataField::Gid,
        &baseline.name,
        baseline.gid.map(i128::from),
        variant.gid.map(i128::from),
        differences,
    );
    compare_value(
        ArchiveMetadataField::Mode,
        &baseline.name,
        baseline.mode.map(i128::from),
        variant.mode.map(i128::from),
        differences,
    );

    if baseline.size != variant.size {
        differences.push(ArchiveMetadataDifference {
            field: ArchiveMetadataField::Size,
            member: Some(baseline.name.clone()),
            baseline: Some(baseline.size.to_string()),
            variant: Some(variant.size.to_string()),
        });
    }
}

fn compare_value(
    field: ArchiveMetadataField,
    member: &str,
    baseline: Option<i128>,
    variant: Option<i128>,
    differences: &mut Vec<ArchiveMetadataDifference>,
) {
    if baseline != variant {
        differences.push(ArchiveMetadataDifference {
            field,
            member: Some(member.to_string()),
            baseline: baseline.map(|value| value.to_string()),
            variant: variant.map(|value| value.to_string()),
        });
    }
}

fn inspect_ar(bytes: &[u8]) -> Option<ArchiveMetadataSummary> {
    if !bytes.starts_with(b"!<arch>\n") {
        return None;
    }

    let mut offset = 8_usize;
    let mut member_count = 0_usize;
    let mut members = Vec::new();
    let mut string_table: Option<&[u8]> = None;

    while offset.checked_add(60)? <= bytes.len() {
        let header = bytes.get(offset..offset + 60)?;
        if header.get(58..60)? != b"`\n" {
            break;
        }

        let raw_name = ascii_field(header.get(0..16)?);
        let mtime = parse_decimal_i64(header.get(16..28)?);
        let uid = parse_decimal_u64(header.get(28..34)?);
        let gid = parse_decimal_u64(header.get(34..40)?);
        let mode = parse_octal_u32(header.get(40..48)?);
        let stored_size = parse_decimal_u64(header.get(48..58)?)?;
        let data_start = offset.checked_add(60)?;
        let stored_size_usize = usize::try_from(stored_size).ok()?;
        let data_end = data_start.checked_add(stored_size_usize)?;
        if data_end > bytes.len() {
            break;
        }

        if raw_name == "//" {
            string_table = bytes.get(data_start..data_end);
        } else if raw_name != "/" && raw_name != "/SYM64/" {
            let (name, logical_size) = resolve_ar_name(
                &raw_name,
                bytes.get(data_start..data_end)?,
                string_table,
                stored_size,
            );
            if members.len() < MAX_RECORDED_MEMBERS {
                members.push(ArchiveMemberMetadata {
                    index: member_count,
                    name,
                    size: logical_size,
                    mtime,
                    uid,
                    gid,
                    mode,
                });
            }
            member_count += 1;
        }

        offset = data_end.checked_add((stored_size_usize % 2) as usize)?;
    }

    Some(ArchiveMetadataSummary {
        format: "ar".to_string(),
        member_count,
        members,
        truncated: member_count > MAX_RECORDED_MEMBERS,
        container_mtime: None,
    })
}

fn resolve_ar_name(
    raw_name: &str,
    data: &[u8],
    string_table: Option<&[u8]>,
    stored_size: u64,
) -> (String, u64) {
    if let Some(length) = raw_name
        .strip_prefix("#1/")
        .and_then(|value| value.trim().parse::<usize>().ok())
    {
        if let Some(name_bytes) = data.get(..length) {
            let name = String::from_utf8_lossy(name_bytes)
                .trim_end_matches('\0')
                .to_string();
            return (name, stored_size.saturating_sub(length as u64));
        }
    }

    if let Some(index) = raw_name
        .strip_prefix('/')
        .and_then(|value| value.trim().parse::<usize>().ok())
    {
        if let Some(table) = string_table {
            if let Some(tail) = table.get(index..) {
                let end = tail
                    .windows(2)
                    .position(|window| window == b"/\n")
                    .or_else(|| tail.iter().position(|byte| *byte == b'\n'))
                    .unwrap_or(tail.len());
                let name = String::from_utf8_lossy(&tail[..end])
                    .trim_end_matches('/')
                    .to_string();
                return (name, stored_size);
            }
        }
    }

    (raw_name.trim_end_matches('/').trim().to_string(), stored_size)
}

fn inspect_tar(bytes: &[u8]) -> Option<ArchiveMetadataSummary> {
    if bytes.len() < 512 {
        return None;
    }

    let mut offset = 0_usize;
    let mut member_count = 0_usize;
    let mut members = Vec::new();
    let mut pending_long_name: Option<String> = None;
    let mut saw_header = false;

    while offset.checked_add(512)? <= bytes.len() {
        let header = bytes.get(offset..offset + 512)?;
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        saw_header = true;

        let size = parse_tar_number(header.get(124..136)?)?;
        let typeflag = *header.get(156)?;
        let data_start = offset.checked_add(512)?;
        let padded_blocks = size.checked_add(511)? / 512;
        let padded = usize::try_from(padded_blocks.checked_mul(512)?).ok()?;
        let data_end = data_start.checked_add(padded)?;
        if data_end > bytes.len() {
            break;
        }

        if typeflag == b'L' {
            let raw_size = usize::try_from(size).ok()?;
            let long_name_end = data_start.checked_add(raw_size)?;
            let raw = bytes.get(data_start..long_name_end)?;
            pending_long_name = Some(
                String::from_utf8_lossy(raw)
                    .trim_end_matches('\0')
                    .trim_end_matches('\n')
                    .to_string(),
            );
            offset = data_end;
            continue;
        }

        if matches!(typeflag, b'x' | b'g' | b'K') {
            offset = data_end;
            continue;
        }

        let name = pending_long_name.take().unwrap_or_else(|| tar_name(header));
        if members.len() < MAX_RECORDED_MEMBERS {
            members.push(ArchiveMemberMetadata {
                index: member_count,
                name,
                size,
                mtime: parse_tar_number(header.get(136..148)?).and_then(|value| i64::try_from(value).ok()),
                uid: parse_tar_number(header.get(108..116)?),
                gid: parse_tar_number(header.get(116..124)?),
                mode: parse_tar_number(header.get(100..108)?).and_then(|value| u32::try_from(value).ok()),
            });
        }
        member_count += 1;
        offset = data_end;
    }

    saw_header.then_some(ArchiveMetadataSummary {
        format: "tar".to_string(),
        member_count,
        members,
        truncated: member_count > MAX_RECORDED_MEMBERS,
        container_mtime: None,
    })
}

fn inspect_zip(bytes: &[u8]) -> Option<ArchiveMetadataSummary> {
    let mut search_offset = 0_usize;
    let mut member_count = 0_usize;
    let mut members = Vec::new();
    let mut saw_central_directory = false;

    while search_offset.checked_add(46)? <= bytes.len() {
        let Some(relative) = bytes
            .get(search_offset..)?
            .windows(4)
            .position(|window| window == b"PK\x01\x02")
        else {
            break;
        };
        let offset = search_offset.checked_add(relative)?;
        let header = bytes.get(offset..offset + 46)?;
        saw_central_directory = true;

        let version_made_by = read_u16_le(header, 4)?;
        let dos_time = read_u16_le(header, 12)?;
        let dos_date = read_u16_le(header, 14)?;
        let uncompressed_size = u64::from(read_u32_le(header, 24)?);
        let name_len = usize::from(read_u16_le(header, 28)?);
        let extra_len = usize::from(read_u16_le(header, 30)?);
        let comment_len = usize::from(read_u16_le(header, 32)?);
        let external_attributes = read_u32_le(header, 38)?;

        let variable_start = offset.checked_add(46)?;
        let name_end = variable_start.checked_add(name_len)?;
        let extra_end = name_end.checked_add(extra_len)?;
        let record_end = extra_end.checked_add(comment_len)?;
        if record_end > bytes.len() {
            break;
        }

        let name = String::from_utf8_lossy(bytes.get(variable_start..name_end)?).to_string();
        let (uid, gid) = parse_zip_unix_ids(bytes.get(name_end..extra_end)?);
        let unix_creator = (version_made_by >> 8) == 3;
        let mode = unix_creator.then_some((external_attributes >> 16) & 0xffff);

        if members.len() < MAX_RECORDED_MEMBERS {
            members.push(ArchiveMemberMetadata {
                index: member_count,
                name,
                size: uncompressed_size,
                mtime: dos_datetime_to_unix(dos_date, dos_time),
                uid,
                gid,
                mode,
            });
        }
        member_count += 1;
        search_offset = record_end;
    }

    saw_central_directory.then_some(ArchiveMetadataSummary {
        format: "zip".to_string(),
        member_count,
        members,
        truncated: member_count > MAX_RECORDED_MEMBERS,
        container_mtime: None,
    })
}

fn inspect_gzip(bytes: &[u8]) -> Option<ArchiveMetadataSummary> {
    if bytes.len() < 10 || !bytes.starts_with(b"\x1f\x8b") {
        return None;
    }
    let raw = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    Some(ArchiveMetadataSummary {
        format: "gzip".to_string(),
        member_count: 1,
        members: Vec::new(),
        truncated: false,
        container_mtime: Some(i64::from(raw)),
    })
}

fn parse_zip_unix_ids(extra: &[u8]) -> (Option<u64>, Option<u64>) {
    let mut offset = 0_usize;
    while offset.checked_add(4).is_some_and(|end| end <= extra.len()) {
        let Some(kind) = read_u16_le(extra, offset) else {
            break;
        };
        let Some(size) = read_u16_le(extra, offset + 2) else {
            break;
        };
        let data_start = offset + 4;
        let data_end = data_start.saturating_add(usize::from(size));
        if data_end > extra.len() {
            break;
        }

        if kind == 0x7875 {
            let data = &extra[data_start..data_end];
            if data.first() == Some(&1) && data.len() >= 3 {
                let uid_size = usize::from(data[1]);
                let uid_start = 2_usize;
                let uid_end = uid_start.saturating_add(uid_size);
                if uid_end < data.len() {
                    let gid_size = usize::from(data[uid_end]);
                    let gid_start = uid_end + 1;
                    let gid_end = gid_start.saturating_add(gid_size);
                    if gid_end <= data.len() {
                        return (
                            little_endian_integer(&data[uid_start..uid_end]),
                            little_endian_integer(&data[gid_start..gid_end]),
                        );
                    }
                }
            }
        }

        offset = data_end;
    }
    (None, None)
}

fn little_endian_integer(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    let mut value = 0_u64;
    for (shift, byte) in bytes.iter().enumerate() {
        value |= u64::from(*byte) << (shift * 8);
    }
    Some(value)
}

fn dos_datetime_to_unix(date: u16, time: u16) -> Option<i64> {
    let year = 1980_i64 + i64::from((date >> 9) & 0x7f);
    let month = i64::from((date >> 5) & 0x0f);
    let day = i64::from(date & 0x1f);
    let hour = i64::from((time >> 11) & 0x1f);
    let minute = i64::from((time >> 5) & 0x3f);
    let second = i64::from(time & 0x1f) * 2;

    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }

    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = year - if month <= 2 { 1 } else { 0 };
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn tar_name(header: &[u8]) -> String {
    let name = nul_terminated(header.get(0..100).unwrap_or_default());
    let prefix = nul_terminated(header.get(345..500).unwrap_or_default());
    if prefix.is_empty() {
        name
    } else if name.is_empty() {
        prefix
    } else {
        format!("{prefix}/{name}")
    }
}

fn nul_terminated(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

fn parse_tar_number(bytes: &[u8]) -> Option<u64> {
    if bytes.first().is_some_and(|byte| *byte & 0x80 != 0) {
        let mut value = 0_u64;
        for (index, byte) in bytes.iter().enumerate() {
            let normalized = if index == 0 { *byte & 0x7f } else { *byte };
            value = value.checked_mul(256)?.checked_add(u64::from(normalized))?;
        }
        return Some(value);
    }
    parse_octal_u64(bytes)
}

fn parse_decimal_i64(bytes: &[u8]) -> Option<i64> {
    ascii_field(bytes).parse::<i64>().ok()
}

fn parse_decimal_u64(bytes: &[u8]) -> Option<u64> {
    ascii_field(bytes).parse::<u64>().ok()
}

fn parse_octal_u32(bytes: &[u8]) -> Option<u32> {
    let field = ascii_field(bytes);
    let trimmed = field.trim_start_matches('0');
    if trimmed.is_empty() {
        Some(0)
    } else {
        u32::from_str_radix(trimmed, 8).ok()
    }
}

fn parse_octal_u64(bytes: &[u8]) -> Option<u64> {
    let field = ascii_field(bytes);
    let trimmed = field.trim_start_matches('0');
    if trimmed.is_empty() {
        Some(0)
    } else {
        u64::from_str_radix(trimmed, 8).ok()
    }
}

fn ascii_field(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_matches(|ch: char| ch == '\0' || ch.is_ascii_whitespace())
        .to_string()
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ar_header(name: &str, mtime: i64, mode: u32, data: &[u8]) -> Vec<u8> {
        let header = format!(
            "{:<16}{:<12}{:<6}{:<6}{:<8o}{:<10}`\n",
            name,
            mtime,
            1000,
            1000,
            mode,
            data.len()
        );
        assert_eq!(header.len(), 60);
        let mut bytes = header.into_bytes();
        bytes.extend_from_slice(data);
        if data.len() % 2 == 1 {
            bytes.push(b'\n');
        }
        bytes
    }

    #[test]
    fn parses_ar_member_metadata() {
        let mut bytes = b"!<arch>\n".to_vec();
        bytes.extend(ar_header("member.txt/", 946684800, 0o644, b"hello"));
        let summary = inspect_ar(&bytes).unwrap();
        assert_eq!(summary.member_count, 1);
        assert_eq!(summary.members[0].name, "member.txt");
        assert_eq!(summary.members[0].mtime, Some(946684800));
        assert_eq!(summary.members[0].mode, Some(0o644));
    }

    #[test]
    fn archive_compare_finds_structural_mtime_change() {
        let baseline = ArchiveMetadataSummary {
            format: "ar".into(),
            member_count: 1,
            members: vec![ArchiveMemberMetadata {
                index: 0,
                name: "a.o".into(),
                size: 3,
                mtime: Some(1),
                uid: Some(0),
                gid: Some(0),
                mode: Some(0o644),
            }],
            truncated: false,
            container_mtime: None,
        };
        let mut variant = baseline.clone();
        variant.members[0].mtime = Some(2);
        let differences = compare(&baseline, &variant);
        assert!(
            differences
                .iter()
                .any(|difference| difference.field == ArchiveMetadataField::Mtime)
        );
    }

    #[test]
    fn converts_dos_epoch_boundary() {
        let date: u16 = (1 << 5) | 1; // 1980-01-01
        assert_eq!(dos_datetime_to_unix(date, 0), Some(315_532_800));
    }
}
