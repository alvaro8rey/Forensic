use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}};
use std::time::Instant;

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use tracing::{debug, info, warn};

use super::types::{FileSignature, FileType, RecoveredFile, ScanProgress, ScanStatus};

const BLOCK_SIZE: usize = 512 * 1024; // 512 KB read blocks
const SECTOR_SIZE: u64 = 512;
/// Overlap between consecutive blocks so signatures spanning a block boundary
/// are not missed.  Must be >= longest (header + verify_offset + verify_len).
/// AVI/WAV need 4 (RIFF) + 8 + 4 = 16, BMP needs 2 + 6 + 4 = 12 → 32 is safe.
const OVERLAP_SIZE: usize = 32;

pub struct FileCarver {
    device_path: String,
    progress_tx: Sender<ScanProgress>,
    cancel_flag: Arc<AtomicBool>,
    bytes_scanned: Arc<AtomicU64>,
    found_count: Arc<AtomicU64>,
    /// When `Some`, only signatures whose `file_type` is in this list are used.
    /// `None` means scan for all types (full scan).
    allowed_types: Option<Vec<FileType>>,
}

impl FileCarver {
    pub fn new(
        device_path: String,
        progress_tx: Sender<ScanProgress>,
        cancel_flag: Arc<AtomicBool>,
        allowed_types: Option<Vec<FileType>>,
    ) -> Self {
        Self {
            device_path,
            progress_tx,
            cancel_flag,
            bytes_scanned: Arc::new(AtomicU64::new(0)),
            found_count: Arc::new(AtomicU64::new(0)),
            allowed_types,
        }
    }

    pub fn scan(&self) -> Result<Vec<RecoveredFile>> {
        info!("Starting file carving on: {}", self.device_path);

        let mut file = self.open_device()?;
        let total_size = self.get_device_size(&mut file)?;

        // Filter signatures to only the allowed types (if a profile was set)
        let signatures: Vec<FileSignature> = FileSignature::all_signatures()
            .into_iter()
            .filter(|s| {
                self.allowed_types.as_ref()
                    .map(|allowed| allowed.contains(&s.file_type))
                    .unwrap_or(true)
            })
            .collect();

        let scan_txt = self.allowed_types.as_ref()
            .map(|t| t.contains(&FileType::TXT))
            .unwrap_or(true);
        let mut recovered: Vec<RecoveredFile> = Vec::new();
        let mut file_id: u64 = 0;
        let start_time = Instant::now();

        let mut buffer = vec![0u8; BLOCK_SIZE + OVERLAP_SIZE];
        let mut offset: u64 = 0;
        let mut tail_overlap = vec![0u8; OVERLAP_SIZE];

        info!("Device size: {} bytes ({:.2} GB)", total_size, total_size as f64 / 1e9);

        while offset < total_size {
            if self.cancel_flag.load(Ordering::Relaxed) {
                info!("Scan cancelled at offset 0x{:X}", offset);
                break;
            }

            // Prepend tail overlap from previous block
            buffer[..OVERLAP_SIZE].copy_from_slice(&tail_overlap);

            let read_size = std::cmp::min(BLOCK_SIZE, (total_size - offset) as usize);
            match file.read(&mut buffer[OVERLAP_SIZE..OVERLAP_SIZE + read_size]) {
                Ok(0) => break,
                Ok(n) => {
                    let window = &buffer[..OVERLAP_SIZE + n];

                    // ── Signature-based carving ───────────────────────────────
                    for sig in &signatures {
                        let matches = find_signature_offsets(window, &sig.header);
                        for local_offset in matches {
                            // Secondary verify check (e.g. RIFF→AVI vs WAV, BMP reserved)
                            if let Some((v_off, v_bytes)) = &sig.verify {
                                let v_start = local_offset + v_off;
                                let v_end = v_start + v_bytes.len();
                                if v_end > window.len()
                                    || &window[v_start..v_end] != v_bytes.as_slice()
                                {
                                    continue; // verification failed
                                }
                            }

                            // Compute absolute device offset.
                            // Unified formula works for both the first block (offset=0,
                            // where the first OVERLAP_SIZE bytes are zeros) and all
                            // subsequent blocks.
                            let abs_offset = (offset as i64
                                - OVERLAP_SIZE as i64
                                + local_offset as i64)
                                .max(0) as u64;

                            debug!(
                                "Found {:?} signature at 0x{:016X}",
                                sig.file_type, abs_offset
                            );

                            let (end_offset, is_fragmented) = self.find_file_end(
                                &mut file,
                                abs_offset,
                                &sig.footer,
                                sig.max_size,
                                total_size,
                            );

                            let size = end_offset.saturating_sub(abs_offset);
                            let recovery_prob = self.calculate_recovery_probability(
                                abs_offset, size, is_fragmented,
                            );

                            let preview_available = matches!(
                                sig.file_type,
                                FileType::JPEG
                                    | FileType::PNG
                                    | FileType::GIF
                                    | FileType::BMP
                                    | FileType::TIFF
                                    | FileType::TXT
                            );

                            recovered.push(RecoveredFile {
                                id: file_id,
                                file_type: sig.file_type.clone(),
                                offset_start: abs_offset,
                                offset_end: end_offset,
                                size_bytes: size,
                                recovery_probability: recovery_prob,
                                signature_matched: format!(
                                    "{:02X?}",
                                    &sig.header[..sig.header.len().min(4)]
                                ),
                                is_fragmented,
                                fragment_count: if is_fragmented { 2 } else { 1 },
                                sector_overwritten: recovery_prob < 0.3,
                                preview_available,
                                thumbnail_base64: None,
                                original_name: None,
                            });

                            file_id += 1;
                            self.found_count.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    // ── Text-file detection ───────────────────────────────────
                    // Check each 512-byte sector in the newly-read portion of the
                    // window.  When a sector looks like plain text we measure how
                    // far the text continues and record it as a TXT file.
                    // Skip the very first 4 KB of the device (MBR/VBR metadata).
                    let mut ts = OVERLAP_SIZE;
                    if !scan_txt { ts = OVERLAP_SIZE + n; } // skip TXT detection if not requested
                    while ts + 512 <= OVERLAP_SIZE + n {
                        let sector = &buffer[ts..ts + 512];
                        if is_text_block(sector) {
                            let abs_txt = (offset as i64 - OVERLAP_SIZE as i64 + ts as i64)
                                .max(0) as u64;

                            let in_metadata = abs_txt < 4096;
                            let already_covered = recovered.iter().any(|f| {
                                f.file_type == FileType::TXT
                                    && abs_txt >= f.offset_start
                                    && abs_txt < f.offset_end
                            });

                            if !in_metadata && !already_covered {
                                let txt_end = find_text_end(&mut file, abs_txt, total_size);
                                let txt_size = txt_end.saturating_sub(abs_txt);

                                if txt_size >= 128 {
                                    let prob = self.calculate_recovery_probability(
                                        abs_txt, txt_size, false,
                                    );
                                    recovered.push(RecoveredFile {
                                        id: file_id,
                                        file_type: FileType::TXT,
                                        offset_start: abs_txt,
                                        offset_end: txt_end,
                                        size_bytes: txt_size,
                                        recovery_probability: prob,
                                        signature_matched: "TEXT".to_string(),
                                        is_fragmented: false,
                                        fragment_count: 1,
                                        sector_overwritten: false,
                                        preview_available: true,
                                        thumbnail_base64: None,
                                        original_name: None,
                                    });
                                    file_id += 1;
                                    self.found_count.fetch_add(1, Ordering::Relaxed);

                                    // Jump past the detected text block
                                    let skip_bytes = txt_size as usize;
                                    ts += ((skip_bytes / 512) + 1) * 512;
                                    continue;
                                }
                            }
                        }
                        ts += 512;
                    }

                    // Save tail for next iteration overlap
                    let tail_start = if n >= OVERLAP_SIZE { n - OVERLAP_SIZE } else { 0 };
                    tail_overlap.copy_from_slice(
                        &buffer[OVERLAP_SIZE + tail_start..OVERLAP_SIZE + tail_start + OVERLAP_SIZE],
                    );

                    offset += n as u64;
                    self.bytes_scanned.store(offset, Ordering::Relaxed);

                    // Restore cursor for next read (find_file_end / find_text_end
                    // may have moved it).
                    file.seek(SeekFrom::Start(offset))?;

                    // Emit progress
                    let elapsed = start_time.elapsed().as_secs();
                    let speed = if elapsed > 0 {
                        (offset as f64 / 1024.0 / 1024.0) / elapsed as f64
                    } else {
                        0.0
                    };

                    let _ = self.progress_tx.try_send(ScanProgress {
                        bytes_scanned: offset,
                        total_bytes: total_size,
                        current_offset_hex: format!("0x{:016X}", offset),
                        files_found: self.found_count.load(Ordering::Relaxed) as u32,
                        scan_speed_mb: speed,
                        elapsed_seconds: elapsed,
                        status: ScanStatus::Scanning,
                    });
                }
                Err(e) => {
                    warn!("Read error at offset 0x{:X}: {}", offset, e);
                    offset += SECTOR_SIZE;
                    file.seek(SeekFrom::Start(offset))?;
                }
            }
        }

        // Deduplicate by (type, offset) — the overlap window can produce two
        // detections for the same signature at the same absolute position.
        let before = recovered.len();
        let mut seen = std::collections::HashSet::new();
        recovered.retain(|f| {
            let key = format!("{:?}:{}", f.file_type, f.offset_start);
            seen.insert(key)
        });
        for (i, f) in recovered.iter_mut().enumerate() {
            f.id = i as u64;
        }

        // Post-process: disambiguate ZIP-based formats.
        // All ZIP-signature matches start as DOCX; refine to XLSX/PPTX/ZIP
        // by inspecting the first local file header filenames (uncompressed ASCII).
        for f in recovered.iter_mut() {
            if f.file_type == FileType::DOCX {
                f.file_type = detect_zip_subtype(&mut file, f.offset_start);
            }
        }

        info!(
            "Scan complete. {} files found ({} duplicates removed).",
            recovered.len(),
            before - recovered.len()
        );
        Ok(recovered)
    }

    fn find_file_end(
        &self,
        file: &mut std::fs::File,
        start: u64,
        footer: &Option<Vec<u8>>,
        max_size: u64,
        total_size: u64,
    ) -> (u64, bool) {
        let footer = match footer {
            None => return ((start + max_size).min(total_size), false),
            Some(f) => f,
        };

        let search_end = (start + max_size).min(total_size);
        let mut scan_buf = vec![0u8; BLOCK_SIZE];
        let mut pos = start;
        let mut fragmented = false;
        let mut last_good_pos = start;
        let mut gap_count = 0u32;

        let _ = file.seek(SeekFrom::Start(start));

        while pos < search_end {
            let to_read = std::cmp::min(BLOCK_SIZE as u64, search_end - pos) as usize;
            match file.read(&mut scan_buf[..to_read]) {
                Ok(0) => break,
                Ok(n) => {
                    // Null-byte gap detection → fragmentation heuristic
                    let null_run =
                        scan_buf[..n].windows(512).any(|w| w.iter().all(|&b| b == 0));
                    if null_run && pos > start + SECTOR_SIZE {
                        gap_count += 1;
                        if gap_count > 2 {
                            fragmented = true;
                        }
                    }

                    if let Some(found) = scan_buf[..n]
                        .windows(footer.len())
                        .position(|w| w == footer.as_slice())
                    {
                        let end = pos + found as u64 + footer.len() as u64;
                        let _ = file.seek(SeekFrom::Start(end));
                        return (end, fragmented);
                    }

                    last_good_pos = pos + n as u64;
                    pos += n as u64;
                }
                Err(_) => {
                    pos += SECTOR_SIZE;
                    let _ = file.seek(SeekFrom::Start(pos));
                }
            }
        }

        let _ = file.seek(SeekFrom::Start(last_good_pos));
        (last_good_pos, true)
    }

    fn calculate_recovery_probability(
        &self,
        offset: u64,
        size: u64,
        is_fragmented: bool,
    ) -> f32 {
        let mut prob: f32 = 1.0;

        if is_fragmented {
            prob -= 0.35;
        }
        if size < 4096 {
            prob += 0.1;
        } else if size > 50 * 1024 * 1024 {
            prob -= 0.2;
        }
        if offset < 1024 * 1024 * 1024 {
            prob -= 0.15;
        }

        prob.clamp(0.05, 1.0)
    }

    #[cfg(target_os = "windows")]
    fn open_device(&self) -> Result<std::fs::File> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
            .open(&self.device_path)
            .context(format!("Failed to open device: {}", self.device_path))
    }

    #[cfg(not(target_os = "windows"))]
    fn open_device(&self) -> Result<std::fs::File> {
        std::fs::OpenOptions::new()
            .read(true)
            .open(&self.device_path)
            .context(format!("Failed to open device: {}", self.device_path))
    }

    #[cfg(target_os = "windows")]
    fn get_device_size(&self, file: &mut std::fs::File) -> Result<u64> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::IO::DeviceIoControl;
        use windows_sys::Win32::System::Ioctl::IOCTL_DISK_GET_LENGTH_INFO;

        let handle = file.as_raw_handle() as isize;
        let mut length: u64 = 0;
        let mut bytes_returned: u32 = 0;

        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_DISK_GET_LENGTH_INFO,
                std::ptr::null(),
                0,
                &mut length as *mut u64 as *mut _,
                8,
                &mut bytes_returned,
                std::ptr::null_mut(),
            )
        };

        if ok != 0 && bytes_returned >= 8 && length > 0 {
            file.seek(SeekFrom::Start(0))?;
            return Ok(length);
        }

        let size = file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;
        Ok(size)
    }

    #[cfg(not(target_os = "windows"))]
    fn get_device_size(&self, file: &mut std::fs::File) -> Result<u64> {
        let size = file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;
        Ok(size)
    }
}

// ── Free helpers ─────────────────────────────────────────────────────────────

/// Reads up to 2 KB from `offset` and checks for Office/ZIP filename markers
/// in the raw (uncompressed) local file header filenames.
fn detect_zip_subtype(file: &mut std::fs::File, offset: u64) -> FileType {
    let _ = file.seek(SeekFrom::Start(offset));
    let mut buf = [0u8; 2048];
    let n = file.read(&mut buf).unwrap_or(0);
    let data = &buf[..n];

    let has = |pat: &[u8]| data.windows(pat.len()).any(|w| w == pat);

    // Office format markers appear as raw ASCII in local file header names
    if has(b"word/")  { return FileType::DOCX; }
    if has(b"xl/")    { return FileType::XLSX; }
    if has(b"ppt/")   { return FileType::PPTX; }

    FileType::ZIP
}

/// Boyer-Moore-Horspool simplified: finds all occurrences of `needle` in `haystack`.
fn find_signature_offsets(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return vec![];
    }
    let mut positions = Vec::new();
    let mut i = 0;
    while i <= haystack.len() - needle.len() {
        if &haystack[i..i + needle.len()] == needle {
            positions.push(i);
        }
        i += 1;
    }
    positions
}

/// Returns `true` if the first 128 bytes of `data` are ≥ 85% printable ASCII.
fn is_text_block(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    let check_len = data.len().min(128);
    let printable = data[..check_len]
        .iter()
        .filter(|&&b| matches!(b, b'\t' | b'\n' | b'\r' | 0x20..=0x7E))
        .count();
    (printable as f64 / check_len as f64) >= 0.85
}

/// Reads forward from `start` until printable-ASCII density drops below 60%,
/// the end-of-device is reached, or 10 MB have been consumed.
/// Returns the byte offset just past the last printable character.
fn find_text_end(file: &mut std::fs::File, start: u64, total_size: u64) -> u64 {
    const MAX_TXT: u64 = 10 * 1024 * 1024;
    let limit = (start + MAX_TXT).min(total_size);
    let mut buf = vec![0u8; 4096];
    let mut pos = start;
    let _ = file.seek(SeekFrom::Start(start));

    while pos < limit {
        let to_read = (limit - pos).min(4096) as usize;
        match file.read(&mut buf[..to_read]) {
            Ok(0) => break,
            Ok(n) => {
                let printable = buf[..n]
                    .iter()
                    .filter(|&&b| matches!(b, b'\t' | b'\n' | b'\r' | 0x20..=0x7E))
                    .count();
                if (printable as f64 / n as f64) < 0.60 {
                    // Find the last printable byte in this chunk
                    let end_in_chunk = buf[..n]
                        .iter()
                        .rposition(|&b| matches!(b, b'\t' | b'\n' | b'\r' | 0x20..=0x7E))
                        .map(|p| p + 1)
                        .unwrap_or(0);
                    return pos + end_in_chunk as u64;
                }
                pos += n as u64;
            }
            Err(_) => break,
        }
    }
    pos
}

// ── Fragment reassembly ───────────────────────────────────────────────────────

pub struct FragmentReassembler;

impl FragmentReassembler {
    pub fn reassemble(
        file: &mut std::fs::File,
        fragments: &[(u64, u64)],
    ) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        for &(offset, size) in fragments {
            let mut buf = vec![0u8; size as usize];
            file.seek(SeekFrom::Start(offset))?;
            file.read_exact(&mut buf)?;
            output.extend_from_slice(&buf);
        }
        Ok(output)
    }

    pub fn shannon_entropy(data: &[u8]) -> f64 {
        if data.is_empty() {
            return 0.0;
        }
        let mut counts = [0u64; 256];
        for &b in data {
            counts[b as usize] += 1;
        }
        let len = data.len() as f64;
        counts.iter().filter(|&&c| c > 0).fold(0.0, |acc, &c| {
            let p = c as f64 / len;
            acc - p * p.log2()
        })
    }
}
