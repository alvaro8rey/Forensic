/// FAT32 and exFAT directory carver.
///
/// Scans directory structures for deleted entries (0xE5 marker on FAT32,
/// cleared bit-7 type codes on exFAT) and reconstructs filenames from LFN
/// records.  Works with volume-level device paths (e.g. `\\.\D:` on Windows,
/// `/dev/sdb1` on Linux) where offset 0 is the VBR.
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};

use anyhow::Result;
use tracing::{debug, info, warn};

use super::types::{FileType, RecoveredFile};

// ── Filesystem detection ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum FsType {
    Fat32,
    ExFat,
    Unknown,
}

fn detect_fs_type(boot: &[u8]) -> FsType {
    if boot.len() < 90 {
        return FsType::Unknown;
    }
    // exFAT: OEM name at offset 3 is "EXFAT   "
    if &boot[3..11] == b"EXFAT   " {
        return FsType::ExFat;
    }
    // FAT32: FS type string at offset 82 (0x52)
    if &boot[82..90] == b"FAT32   " {
        return FsType::Fat32;
    }
    // FAT32 fallback: BPB_FATSz16 == 0, BPB_FATSz32 != 0, BPB_RootEntCnt == 0
    let fat16_sz = u16::from_le_bytes([boot[22], boot[23]]);
    let fat32_sz = u32::from_le_bytes([boot[36], boot[37], boot[38], boot[39]]);
    let root_ent = u16::from_le_bytes([boot[17], boot[18]]);
    if fat16_sz == 0 && fat32_sz != 0 && root_ent == 0 {
        return FsType::Fat32;
    }
    FsType::Unknown
}

// ── FAT32 parameters ─────────────────────────────────────────────────────────

struct Fat32Params {
    cluster_size: u64,
    fat_start: u64,   // byte offset of FAT[0] on the device
    data_start: u64,  // byte offset of first data cluster
    root_cluster: u32,
}

fn parse_fat32_params(boot: &[u8]) -> Option<Fat32Params> {
    if boot.len() < 80 {
        return None;
    }
    let bps = u16::from_le_bytes([boot[11], boot[12]]) as u64; // bytes per sector
    let spc = boot[13] as u64;                                   // sectors per cluster
    let rsc = u16::from_le_bytes([boot[14], boot[15]]) as u64;  // reserved sectors
    let nf = boot[16] as u64;                                    // number of FATs
    let fsz = u32::from_le_bytes([boot[36], boot[37], boot[38], boot[39]]) as u64; // FAT size
    let root = u32::from_le_bytes([boot[44], boot[45], boot[46], boot[47]]);       // root cluster

    if bps < 512 || spc == 0 || fsz == 0 {
        return None;
    }
    Some(Fat32Params {
        cluster_size: spc * bps,
        fat_start: rsc * bps,
        data_start: (rsc + nf * fsz) * bps,
        root_cluster: root,
    })
}

#[inline]
fn fat32_cluster_offset(cluster: u32, p: &Fat32Params) -> u64 {
    p.data_start + (cluster as u64 - 2) * p.cluster_size
}

fn fat32_next_cluster(file: &mut std::fs::File, cluster: u32, p: &Fat32Params) -> Option<u32> {
    let off = p.fat_start + cluster as u64 * 4;
    file.seek(SeekFrom::Start(off)).ok()?;
    let mut buf = [0u8; 4];
    file.read_exact(&mut buf).ok()?;
    let next = u32::from_le_bytes(buf) & 0x0FFF_FFFF;
    if (2..0x0FFF_FFF7).contains(&next) { Some(next) } else { None }
}

fn fat32_cluster_chain(
    file: &mut std::fs::File,
    start: u32,
    p: &Fat32Params,
    max: usize,
) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut cur = start;
    while chain.len() < max && (2..0x0FFF_FFF7).contains(&cur) {
        if !seen.insert(cur) {
            break;
        }
        chain.push(cur);
        match fat32_next_cluster(file, cur, p) {
            Some(n) => cur = n,
            None => break,
        }
    }
    chain
}

// ── LFN reconstruction ────────────────────────────────────────────────────────

/// Reconstruct a long filename from a slice of raw 32-byte LFN entries.
///
/// The entries are given in the ORDER they appear in the directory
/// (highest sequence number first, lowest last).  The function reverses them
/// internally so characters are concatenated from position 1 onwards.
fn reconstruct_lfn(entries: &[[u8; 32]]) -> String {
    let mut chars: Vec<u16> = Vec::with_capacity(entries.len() * 13);
    // entries[last] = seq 1 (chars 1–13), entries[0] = seq N (last chars)
    for entry in entries.iter().rev() {
        let slices: &[(usize, usize)] = &[(1, 5), (14, 6), (28, 2)];
        'outer: for &(start, count) in slices {
            for i in 0..count {
                let p = start + i * 2;
                if p + 1 >= 32 {
                    break 'outer;
                }
                let ch = u16::from_le_bytes([entry[p], entry[p + 1]]);
                if ch == 0x0000 || ch == 0xFFFF {
                    return String::from_utf16_lossy(&chars).to_string();
                }
                chars.push(ch);
            }
        }
    }
    String::from_utf16_lossy(&chars).to_string()
}

/// Build a plain 8.3 name (ASCII) from a directory entry, replacing the
/// 0xE5 first byte with '_'.
fn build_83_name(raw: &[u8; 32]) -> String {
    let mut name_bytes = [0u8; 8];
    name_bytes.copy_from_slice(&raw[0..8]);
    if name_bytes[0] == 0xE5 {
        name_bytes[0] = b'_';
    }
    let name: String = name_bytes
        .iter()
        .take_while(|&&b| b != 0x20 && b != 0x00)
        .map(|&b| b as char)
        .collect();
    let ext: String = raw[8..11]
        .iter()
        .take_while(|&&b| b != 0x20 && b != 0x00)
        .map(|&b| b as char)
        .collect();
    if ext.is_empty() { name } else { format!("{}.{}", name, ext) }
}

// ── FAT32 directory scanner ───────────────────────────────────────────────────

fn scan_fat32_dir(
    file: &mut std::fs::File,
    start_cluster: u32,
    params: &Fat32Params,
    depth: u32,
    results: &mut Vec<RecoveredFile>,
    visited: &mut HashSet<u32>,
    next_id: &mut u64,
) {
    if depth > 8 || !visited.insert(start_cluster) {
        return;
    }

    let chain = fat32_cluster_chain(file, start_cluster, params, 8192);
    let cluster_size = params.cluster_size as usize;
    let mut lfn_buf: Vec<[u8; 32]> = Vec::new();

    for cluster in chain {
        let cluster_off = fat32_cluster_offset(cluster, params);
        let mut cluster_data = vec![0u8; cluster_size];

        if file.seek(SeekFrom::Start(cluster_off)).is_err()
            || file.read_exact(&mut cluster_data).is_err()
        {
            lfn_buf.clear();
            continue;
        }

        let count = cluster_data.len() / 32;
        for i in 0..count {
            let raw: &[u8; 32] = cluster_data[i * 32..(i + 1) * 32].try_into().unwrap();
            let first = raw[0];
            let attrs = raw[11];

            // End of directory
            if first == 0x00 {
                lfn_buf.clear();
                return;
            }

            // LFN entry (attribute 0x0F)
            if attrs == 0x0F {
                lfn_buf.push(*raw);
                continue;
            }

            // Volume label / special — discard accumulated LFNs
            if attrs & 0x08 != 0 {
                lfn_buf.clear();
                continue;
            }

            // ── Deleted entry (0xE5) ──────────────────────────────────────
            if first == 0xE5 {
                // Skip deleted sub-directories (cluster chain is freed)
                if attrs & 0x10 != 0 {
                    lfn_buf.clear();
                    continue;
                }

                let name = if !lfn_buf.is_empty() {
                    reconstruct_lfn(&lfn_buf)
                } else {
                    build_83_name(raw)
                };
                lfn_buf.clear();

                if name.is_empty() || name.trim_matches('_').is_empty() {
                    continue;
                }

                let hi = u16::from_le_bytes([raw[20], raw[21]]) as u32;
                let lo = u16::from_le_bytes([raw[26], raw[27]]) as u32;
                let first_cluster = (hi << 16) | lo;
                let file_size =
                    u32::from_le_bytes([raw[28], raw[29], raw[30], raw[31]]) as u64;

                // Windows preserves first_cluster on deletion; macOS/Linux may zero it
                let offset_start = if first_cluster >= 2 {
                    fat32_cluster_offset(first_cluster, params)
                } else {
                    0
                };
                let offset_end = offset_start + file_size.max(1);
                let recoverable = first_cluster >= 2 && file_size > 0;
                let prob: f32 = if recoverable { 0.72 } else { 0.20 };
                let ftype = ext_to_filetype(&name);
                let preview = is_previewable(&ftype);

                results.push(RecoveredFile {
                    id: *next_id,
                    file_type: ftype,
                    offset_start,
                    offset_end,
                    size_bytes: file_size,
                    recovery_probability: prob,
                    signature_matched: "FAT32_DIR".to_string(),
                    is_fragmented: false,
                    fragment_count: 1,
                    sector_overwritten: !recoverable,
                    preview_available: preview,
                    thumbnail_base64: None,
                    original_name: Some(name),
                });
                *next_id += 1;
                continue;
            }

            // ── Active sub-directory → recurse ───────────────────────────
            if attrs & 0x10 != 0 && raw[0] != b'.' {
                lfn_buf.clear();
                let hi = u16::from_le_bytes([raw[20], raw[21]]) as u32;
                let lo = u16::from_le_bytes([raw[26], raw[27]]) as u32;
                let sub_cluster = (hi << 16) | lo;
                if sub_cluster >= 2 {
                    scan_fat32_dir(file, sub_cluster, params, depth + 1, results, visited, next_id);
                }
                continue;
            }

            lfn_buf.clear();
        }
    }
}

// ── exFAT parameters ─────────────────────────────────────────────────────────

struct ExFatParams {
    cluster_size: u64,
    fat_start: u64,
    data_start: u64,
    root_cluster: u32,
}

fn parse_exfat_params(boot: &[u8]) -> Option<ExFatParams> {
    if boot.len() < 512 || &boot[3..11] != b"EXFAT   " {
        return None;
    }
    let fat_off = u32::from_le_bytes([boot[80], boot[81], boot[82], boot[83]]) as u64;
    let heap_off = u32::from_le_bytes([boot[88], boot[89], boot[90], boot[91]]) as u64;
    let root = u32::from_le_bytes([boot[96], boot[97], boot[98], boot[99]]);
    let bps_shift = boot[108] as u32;
    let spc_shift = boot[109] as u32;

    if !(9..=12).contains(&bps_shift) {
        return None; // bytes/sector must be 512–4096
    }
    let bps = 1u64 << bps_shift;
    let cluster_size = (1u64 << spc_shift) * bps;
    Some(ExFatParams {
        cluster_size,
        fat_start: fat_off * bps,
        data_start: heap_off * bps,
        root_cluster: root,
    })
}

#[inline]
fn exfat_cluster_offset(cluster: u32, p: &ExFatParams) -> u64 {
    p.data_start + (cluster as u64 - 2) * p.cluster_size
}

fn exfat_next_cluster(file: &mut std::fs::File, cluster: u32, p: &ExFatParams) -> Option<u32> {
    let off = p.fat_start + cluster as u64 * 4;
    file.seek(SeekFrom::Start(off)).ok()?;
    let mut buf = [0u8; 4];
    file.read_exact(&mut buf).ok()?;
    let next = u32::from_le_bytes(buf);
    if (2..0xFFFF_FFF7).contains(&next) { Some(next) } else { None }
}

fn exfat_cluster_chain(
    file: &mut std::fs::File,
    start: u32,
    p: &ExFatParams,
    max: usize,
) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut cur = start;
    while chain.len() < max && (2..0xFFFF_FFF7).contains(&cur) {
        if !seen.insert(cur) {
            break;
        }
        chain.push(cur);
        match exfat_next_cluster(file, cur, p) {
            Some(n) => cur = n,
            None => break,
        }
    }
    chain
}

// ── exFAT directory scanner ───────────────────────────────────────────────────

fn scan_exfat_dir(
    file: &mut std::fs::File,
    start_cluster: u32,
    params: &ExFatParams,
    depth: u32,
    results: &mut Vec<RecoveredFile>,
    visited: &mut HashSet<u32>,
    next_id: &mut u64,
) {
    if depth > 8 || !visited.insert(start_cluster) {
        return;
    }

    let chain = exfat_cluster_chain(file, start_cluster, params, 8192);

    for cluster in chain {
        let cluster_off = exfat_cluster_offset(cluster, params);
        let mut data = vec![0u8; params.cluster_size as usize];

        if file.seek(SeekFrom::Start(cluster_off)).is_err()
            || file.read_exact(&mut data).is_err()
        {
            continue;
        }

        let count = data.len() / 32;
        let mut i = 0;

        while i < count {
            let entry = &data[i * 32..(i + 1) * 32];
            let etype = entry[0];

            // Deleted File entry (0x85 → 0x05 when bit7 cleared)
            if etype == 0x05 {
                let sec_count = entry[1] as usize;
                if sec_count < 2 || i + sec_count >= count {
                    i += 1;
                    continue;
                }

                // Stream extension must follow (0xC0 deleted → 0x40)
                let stream = &data[(i + 1) * 32..(i + 2) * 32];
                if stream[0] != 0x40 {
                    i += 1;
                    continue;
                }

                let name_len = stream[3] as usize;
                let valid_len =
                    u64::from_le_bytes(stream[8..16].try_into().unwrap_or([0; 8]));
                let first_cluster =
                    u32::from_le_bytes(stream[20..24].try_into().unwrap_or([0; 4]));
                let data_len =
                    u64::from_le_bytes(stream[24..32].try_into().unwrap_or([0; 8]));

                // Collect name chars from File Name entries (0xC1 → 0x41)
                let mut chars: Vec<u16> = Vec::with_capacity(name_len);
                let mut j = 2;
                while j <= sec_count.min(19) && i + j < count {
                    let fn_e = &data[(i + j) * 32..(i + j + 1) * 32];
                    if fn_e[0] != 0x41 {
                        break;
                    }
                    for k in 0..15usize {
                        let p = 2 + k * 2;
                        if chars.len() >= name_len {
                            break;
                        }
                        let ch = u16::from_le_bytes([fn_e[p], fn_e[p + 1]]);
                        chars.push(ch);
                    }
                    j += 1;
                }

                let name = String::from_utf16_lossy(&chars).to_string();
                if name.is_empty() {
                    i += sec_count + 1;
                    continue;
                }

                let file_size = if valid_len > 0 { valid_len } else { data_len };
                let offset_start = if first_cluster >= 2 {
                    exfat_cluster_offset(first_cluster, params)
                } else {
                    0
                };
                let recoverable = first_cluster >= 2 && file_size > 0;
                let prob: f32 = if recoverable { 0.72 } else { 0.20 };
                let ftype = ext_to_filetype(&name);
                let preview = is_previewable(&ftype);

                results.push(RecoveredFile {
                    id: *next_id,
                    file_type: ftype,
                    offset_start,
                    offset_end: offset_start + file_size.max(1),
                    size_bytes: file_size,
                    recovery_probability: prob,
                    signature_matched: "EXFAT_DIR".to_string(),
                    is_fragmented: false,
                    fragment_count: 1,
                    sector_overwritten: !recoverable,
                    preview_available: preview,
                    thumbnail_base64: None,
                    original_name: Some(name),
                });
                *next_id += 1;

                i += sec_count + 1;
                continue;
            }

            // Active directory (0x85) — recurse into subdirectory
            if etype == 0x85 && i + 1 < count {
                let sec_count = entry[1] as usize;
                let attrs = u16::from_le_bytes([entry[4], entry[5]]);
                if attrs & 0x10 != 0 && sec_count >= 2 {
                    let stream = &data[(i + 1) * 32..(i + 2) * 32];
                    if stream[0] == 0xC0 {
                        let sub =
                            u32::from_le_bytes(stream[20..24].try_into().unwrap_or([0; 4]));
                        if sub >= 2 {
                            scan_exfat_dir(
                                file, sub, params, depth + 1, results, visited, next_id,
                            );
                        }
                    }
                    i += sec_count + 1;
                    continue;
                }
            }

            i += 1;
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn ext_to_filetype(filename: &str) -> FileType {
    let ext = filename
        .rfind('.')
        .map(|i| filename[i + 1..].to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg"              => FileType::JPEG,
        "png"                       => FileType::PNG,
        "gif"                       => FileType::GIF,
        "tif" | "tiff"              => FileType::TIFF,
        "bmp"                       => FileType::BMP,
        "pdf"                       => FileType::PDF,
        "docx"                      => FileType::DOCX,
        "xlsx"                      => FileType::XLSX,
        "pptx"                      => FileType::PPTX,
        "doc" | "xls" | "ppt"      => FileType::DOC,
        "zip"                       => FileType::ZIP,
        "rar"                       => FileType::RAR,
        "7z"                        => FileType::SevenZ,
        "exe" | "dll" | "sys"       => FileType::EXE,
        "mp4" | "m4v" | "mov"      => FileType::MP4,
        "avi"                       => FileType::AVI,
        "mkv" | "webm"             => FileType::MKV,
        "mp3"                       => FileType::MP3,
        "wav"                       => FileType::WAV,
        "flac"                      => FileType::FLAC,
        "sqlite" | "db" | "db3"    => FileType::SQLite,
        "txt" | "log" | "csv"
        | "xml" | "json" | "html"
        | "htm" | "md" | "ini"
        | "cfg" | "bat" | "sh"     => FileType::TXT,
        other if other.is_empty()  => FileType::Unknown("BIN".to_string()),
        other                       => FileType::Unknown(other.to_uppercase()),
    }
}

fn is_previewable(ft: &FileType) -> bool {
    matches!(
        ft,
        FileType::JPEG
            | FileType::PNG
            | FileType::GIF
            | FileType::BMP
            | FileType::TIFF
            | FileType::TXT
    )
}

// ── Public API ────────────────────────────────────────────────────────────────

pub struct FatCarver {
    device_path: String,
}

impl FatCarver {
    pub fn new(device_path: String) -> Self {
        Self { device_path }
    }

    pub fn scan(&self) -> Result<Vec<RecoveredFile>> {
        let mut file = self.open_device()?;

        let mut boot = [0u8; 512];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut boot)?;

        let mut results = Vec::new();
        let mut visited: HashSet<u32> = HashSet::new();
        let mut next_id: u64 = 0;

        match detect_fs_type(&boot) {
            FsType::Fat32 => {
                info!("FAT32 detected on {}", self.device_path);
                match parse_fat32_params(&boot) {
                    Some(params) => {
                        let root = params.root_cluster;
                        scan_fat32_dir(
                            &mut file, root, &params, 0,
                            &mut results, &mut visited, &mut next_id,
                        );
                        info!("FAT32 scan: {} deleted entries", results.len());
                    }
                    None => warn!("Could not parse FAT32 BPB on {}", self.device_path),
                }
            }
            FsType::ExFat => {
                info!("exFAT detected on {}", self.device_path);
                match parse_exfat_params(&boot) {
                    Some(params) => {
                        let root = params.root_cluster;
                        scan_exfat_dir(
                            &mut file, root, &params, 0,
                            &mut results, &mut visited, &mut next_id,
                        );
                        info!("exFAT scan: {} deleted entries", results.len());
                    }
                    None => warn!("Could not parse exFAT BPB on {}", self.device_path),
                }
            }
            FsType::Unknown => {
                debug!("No FAT32/exFAT signature on {} — skipping directory carving", self.device_path);
            }
        }

        Ok(results)
    }

    #[cfg(target_os = "windows")]
    fn open_device(&self) -> Result<std::fs::File> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
        Ok(std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&self.device_path)?)
    }

    #[cfg(not(target_os = "windows"))]
    fn open_device(&self) -> Result<std::fs::File> {
        Ok(std::fs::OpenOptions::new().read(true).open(&self.device_path)?)
    }
}
