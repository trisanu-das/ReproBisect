use std::{fs, path::Path};

use anyhow::{Context, Result};

use crate::model::{ElfMarkerLocation, ElfMetadataSummary, EmbeddedMarker};

const MAX_ELF_ANALYSIS_BYTES: u64 = 128 * 1024 * 1024;
const SHT_NOTE: u32 = 7;
const NT_GNU_BUILD_ID: u32 = 3;

#[derive(Debug, Clone, Copy)]
enum Endian {
    Little,
    Big,
}

#[derive(Debug, Clone, Copy)]
enum ElfClass {
    Elf32,
    Elf64,
}

#[derive(Debug, Clone, Copy)]
struct SectionHeader {
    name_offset: usize,
    section_type: u32,
    file_offset: usize,
    size: usize,
}

#[derive(Debug, Clone)]
struct NamedSection {
    name: String,
    section_type: u32,
    file_offset: usize,
    size: usize,
}

#[derive(Debug, Clone)]
struct ParsedElf {
    endian: Endian,
    sections: Vec<NamedSection>,
}

pub fn scan_debug_markers(path: &Path, markers: &[EmbeddedMarker]) -> Result<Vec<EmbeddedMarker>> {
    let locations = scan_debug_marker_locations(path, markers)?;
    let mut hits = locations
        .into_iter()
        .map(|location| EmbeddedMarker {
            variable: location.variable,
            value: location.value,
        })
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        left.variable
            .cmp(&right.variable)
            .then_with(|| left.value.cmp(&right.value))
    });
    hits.dedup();
    Ok(hits)
}

pub fn scan_debug_marker_locations(
    path: &Path,
    markers: &[EmbeddedMarker],
) -> Result<Vec<ElfMarkerLocation>> {
    if fs::metadata(path)?.len() > MAX_ELF_ANALYSIS_BYTES {
        return Ok(Vec::new());
    }

    let bytes = fs::read(path).with_context(|| format!("cannot read ELF {}", path.display()))?;
    let Some(parsed) = parse_elf(&bytes) else {
        return Ok(Vec::new());
    };

    let mut hits = Vec::new();
    for section in parsed.sections.iter().filter(|section| is_debug_section(&section.name)) {
        let Some(end) = section.file_offset.checked_add(section.size) else {
            continue;
        };
        let Some(section_bytes) = bytes.get(section.file_offset..end) else {
            continue;
        };

        for marker in markers {
            let needle = marker.value.as_bytes();
            if needle.is_empty() {
                continue;
            }
            if contains_bytes(section_bytes, needle) {
                hits.push(ElfMarkerLocation {
                    variable: marker.variable.clone(),
                    value: marker.value.clone(),
                    section: section.name.clone(),
                });
            }
        }
    }

    hits.sort_by(|left, right| {
        left.variable
            .cmp(&right.variable)
            .then_with(|| left.value.cmp(&right.value))
            .then_with(|| left.section.cmp(&right.section))
    });
    hits.dedup();
    Ok(hits)
}

pub fn inspect(path: &Path) -> Result<Option<ElfMetadataSummary>> {
    if fs::metadata(path)?.len() > MAX_ELF_ANALYSIS_BYTES {
        return Ok(None);
    }

    let bytes = fs::read(path).with_context(|| format!("cannot read ELF {}", path.display()))?;
    let Some(parsed) = parse_elf(&bytes) else {
        return Ok(None);
    };

    let mut debug_sections = parsed
        .sections
        .iter()
        .filter(|section| is_debug_section(&section.name))
        .map(|section| section.name.clone())
        .collect::<Vec<_>>();
    debug_sections.sort();
    debug_sections.dedup();

    let build_id = parsed
        .sections
        .iter()
        .filter(|section| section.section_type == SHT_NOTE)
        .filter_map(|section| section_bytes(&bytes, section))
        .find_map(|section_bytes| parse_gnu_build_id(section_bytes, parsed.endian));

    Ok(Some(ElfMetadataSummary {
        build_id,
        debug_sections,
    }))
}

fn parse_elf(bytes: &[u8]) -> Option<ParsedElf> {
    if bytes.get(0..4)? != &b"\x7fELF"[..] {
        return None;
    }

    let class = match *bytes.get(4)? {
        1 => ElfClass::Elf32,
        2 => ElfClass::Elf64,
        _ => return None,
    };
    let endian = match *bytes.get(5)? {
        1 => Endian::Little,
        2 => Endian::Big,
        _ => return None,
    };

    let (section_offset, entry_size, section_count, string_table_index) = match class {
        ElfClass::Elf32 => (
            read_u32(bytes, 0x20, endian)? as usize,
            read_u16(bytes, 0x2e, endian)? as usize,
            read_u16(bytes, 0x30, endian)? as usize,
            read_u16(bytes, 0x32, endian)? as usize,
        ),
        ElfClass::Elf64 => (
            usize::try_from(read_u64(bytes, 0x28, endian)?).ok()?,
            read_u16(bytes, 0x3a, endian)? as usize,
            read_u16(bytes, 0x3c, endian)? as usize,
            read_u16(bytes, 0x3e, endian)? as usize,
        ),
    };

    let minimum_entry_size = match class {
        ElfClass::Elf32 => 40,
        ElfClass::Elf64 => 64,
    };
    if section_offset == 0
        || entry_size < minimum_entry_size
        || section_count == 0
        || string_table_index >= section_count
    {
        return Some(ParsedElf {
            endian,
            sections: Vec::new(),
        });
    }

    let mut raw_sections = Vec::with_capacity(section_count);
    for index in 0..section_count {
        let base = section_offset.checked_add(index.checked_mul(entry_size)?)?;
        let header = match class {
            ElfClass::Elf32 => SectionHeader {
                name_offset: read_u32(bytes, base, endian)? as usize,
                section_type: read_u32(bytes, base.checked_add(0x04)?, endian)?,
                file_offset: read_u32(bytes, base.checked_add(0x10)?, endian)? as usize,
                size: read_u32(bytes, base.checked_add(0x14)?, endian)? as usize,
            },
            ElfClass::Elf64 => SectionHeader {
                name_offset: read_u32(bytes, base, endian)? as usize,
                section_type: read_u32(bytes, base.checked_add(0x04)?, endian)?,
                file_offset: usize::try_from(read_u64(bytes, base.checked_add(0x18)?, endian)?).ok()?,
                size: usize::try_from(read_u64(bytes, base.checked_add(0x20)?, endian)?).ok()?,
            },
        };
        raw_sections.push(header);
    }

    let string_header = *raw_sections.get(string_table_index)?;
    let string_end = string_header.file_offset.checked_add(string_header.size)?;
    let strings = bytes.get(string_header.file_offset..string_end)?;

    let mut sections = Vec::new();
    for section in raw_sections {
        let Some(name) = section_name(strings, section.name_offset) else {
            continue;
        };
        let Some(end) = section.file_offset.checked_add(section.size) else {
            continue;
        };
        if end > bytes.len() {
            continue;
        }
        sections.push(NamedSection {
            name: name.to_string(),
            section_type: section.section_type,
            file_offset: section.file_offset,
            size: section.size,
        });
    }

    Some(ParsedElf { endian, sections })
}

fn section_bytes<'a>(bytes: &'a [u8], section: &NamedSection) -> Option<&'a [u8]> {
    let end = section.file_offset.checked_add(section.size)?;
    bytes.get(section.file_offset..end)
}

fn is_debug_section(name: &str) -> bool {
    name.starts_with(".debug_") || name.starts_with(".zdebug_")
}

fn parse_gnu_build_id(bytes: &[u8], endian: Endian) -> Option<String> {
    let mut offset = 0_usize;
    while offset.checked_add(12)? <= bytes.len() {
        let namesz = read_u32(bytes, offset, endian)? as usize;
        let descsz = read_u32(bytes, offset.checked_add(4)?, endian)? as usize;
        let note_type = read_u32(bytes, offset.checked_add(8)?, endian)?;
        let name_start = offset.checked_add(12)?;
        let name_end = name_start.checked_add(namesz)?;
        let desc_start = align4(name_end)?;
        let desc_end = desc_start.checked_add(descsz)?;
        if desc_end > bytes.len() {
            return None;
        }

        let name = bytes.get(name_start..name_end)?;
        if note_type == NT_GNU_BUILD_ID && name.starts_with(b"GNU") {
            return Some(hex::encode(bytes.get(desc_start..desc_end)?));
        }

        offset = align4(desc_end)?;
    }
    None
}

fn align4(value: usize) -> Option<usize> {
    value.checked_add(3).map(|value| value & !3)
}

fn section_name(strings: &[u8], offset: usize) -> Option<&str> {
    let tail = strings.get(offset..)?;
    let end = tail.iter().position(|byte| *byte == 0).unwrap_or(tail.len());
    std::str::from_utf8(tail.get(..end)?).ok()
}

fn read_u16(bytes: &[u8], offset: usize, endian: Endian) -> Option<u16> {
    let raw: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(match endian {
        Endian::Little => u16::from_le_bytes(raw),
        Endian::Big => u16::from_be_bytes(raw),
    })
}

fn read_u32(bytes: &[u8], offset: usize, endian: Endian) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(match endian {
        Endian::Little => u32::from_le_bytes(raw),
        Endian::Big => u32::from_be_bytes(raw),
    })
}

fn read_u64(bytes: &[u8], offset: usize, endian: Endian) -> Option<u64> {
    let raw: [u8; 8] = bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?;
    Some(match endian {
        Endian::Little => u64::from_le_bytes(raw),
        Endian::Big => u64::from_be_bytes(raw),
    })
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
    fn rejects_non_elf() {
        assert!(parse_elf(b"not an elf").is_none());
    }

    #[test]
    fn parses_minimal_elf64_debug_section_and_location() {
        let mut bytes = vec![0_u8; 64 + 3 * 64];
        bytes[0..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;

        let shoff = 64_u64;
        bytes[0x28..0x30].copy_from_slice(&shoff.to_le_bytes());
        bytes[0x3a..0x3c].copy_from_slice(&64_u16.to_le_bytes());
        bytes[0x3c..0x3e].copy_from_slice(&3_u16.to_le_bytes());
        bytes[0x3e..0x40].copy_from_slice(&1_u16.to_le_bytes());

        let names = b"\0.shstrtab\0.debug_str\0";
        let names_offset = bytes.len();
        bytes.extend_from_slice(names);
        let debug_offset = bytes.len();
        bytes.extend_from_slice(b"prefix /workspace/main.c suffix\0");

        let sh1 = 64 + 64;
        bytes[sh1..sh1 + 4].copy_from_slice(&1_u32.to_le_bytes());
        bytes[sh1 + 0x18..sh1 + 0x20].copy_from_slice(&(names_offset as u64).to_le_bytes());
        bytes[sh1 + 0x20..sh1 + 0x28].copy_from_slice(&(names.len() as u64).to_le_bytes());

        let sh2 = 64 + 2 * 64;
        bytes[sh2..sh2 + 4].copy_from_slice(&11_u32.to_le_bytes());
        bytes[sh2 + 0x18..sh2 + 0x20].copy_from_slice(&(debug_offset as u64).to_le_bytes());
        bytes[sh2 + 0x20..sh2 + 0x28].copy_from_slice(&32_u64.to_le_bytes());

        let parsed = parse_elf(&bytes).unwrap();
        let debug = parsed.sections.iter().find(|section| section.name == ".debug_str").unwrap();
        assert!(contains_bytes(section_bytes(&bytes, debug).unwrap(), b"/workspace"));
    }

    #[test]
    fn parses_gnu_build_id_note() {
        let mut note = Vec::new();
        note.extend_from_slice(&4_u32.to_le_bytes());
        note.extend_from_slice(&4_u32.to_le_bytes());
        note.extend_from_slice(&3_u32.to_le_bytes());
        note.extend_from_slice(b"GNU\0");
        note.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(parse_gnu_build_id(&note, Endian::Little).as_deref(), Some("deadbeef"));
    }
}
