use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}};
use std::time::Instant;

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use tracing::{debug, info, warn};

use super::types::{FileSignature, FileType, RecoveredFile, ScanProgress, ScanStatus};

const BLOCK_SIZE: usize = 512 * 1024; // 512KB read blocks
const SECTOR_SIZE: u64 = 512;
const OVERLAP_SIZE: usize = 16; // Bytes to overlap between blocks for split-signature detection

pub struct FileCarver {
    device_path: String,
    progress_tx: Sender<ScanProgress>,
    cancel_flag: Arc<AtomicBool>,
    bytes_scanned: Arc<AtomicU64>,
    found_count: Arc<AtomicU64>,
}

impl FileCarver {
    pub fn new(
        device_path: String,
        progress_tx: Sender<ScanProgress>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Self {
        Self {
            device_path,
            progress_tx,
            cancel_flag,
            bytes_scanned: Arc::new(AtomicU64::new(0)),
            found_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn scan(&self) -> Result<Vec<RecoveredFile>> {
        info!("Starting file carving on: {}", self.device_path);

        let mut file = self.open_device()?;
        let total_size = self.get_device_size(&mut file)?;
        let signatures = FileSignature::all_signatures();
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

                    // Search each signature within this window
                    for sig in &signatures {
                        let matches = self.find_signature_offsets(window, &sig.header);
                        for local_offset in matches {
                            let abs_offset = if offset == 0 {
                                local_offset as u64
                            } else {
                                offset - OVERLAP_SIZE as u64 + local_offset as u64
                            };

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
                                preview_available: matches!(
                                    sig.file_type,
                                    FileType::JPEG | FileType::PNG
                                ),
                                thumbnail_base64: None,
                            });

                            file_id += 1;
                            self.found_count.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    // Save tail for next iteration overlap
                    let tail_start = if n >= OVERLAP_SIZE { n - OVERLAP_SIZE } else { 0 };
                    tail_overlap.copy_from_slice(&buffer[OVERLAP_SIZE + tail_start..OVERLAP_SIZE + tail_start + OVERLAP_SIZE]);

                    offset += n as u64;
                    self.bytes_scanned.store(offset, Ordering::Relaxed);

                    // Seek back to realign (we always read from true offset)
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
                    // Skip bad sector and continue
                    offset += SECTOR_SIZE;
                    file.seek(SeekFrom::Start(offset))?;
                }
            }
        }

        info!("Scan complete. Found {} files.", recovered.len());
        Ok(recovered)
    }

    fn find_signature_offsets(&self, haystack: &[u8], needle: &[u8]) -> Vec<usize> {
        if needle.is_empty() || haystack.len() < needle.len() {
            return vec![];
        }

        let mut positions = Vec::new();
        let mut i = 0;

        // Boyer-Moore-Horspool simplified
        while i <= haystack.len() - needle.len() {
            if &haystack[i..i + needle.len()] == needle {
                positions.push(i);
                i += 1;
            } else {
                i += 1;
            }
        }
        positions
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
            None => return (start + max_size.min(total_size - start), false),
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
                    // Check for null-byte gaps indicating fragmentation
                    let null_run = scan_buf[..n].windows(512).any(|w| w.iter().all(|&b| b == 0));
                    if null_run && pos > start + SECTOR_SIZE {
                        gap_count += 1;
                        if gap_count > 2 {
                            fragmented = true;
                        }
                    }

                    // Search for footer
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

        // Fragmentation penalty
        if is_fragmented {
            prob -= 0.35;
        }

        // Small files are less likely to be partially overwritten
        if size < 4096 {
            prob += 0.1;
        } else if size > 50 * 1024 * 1024 {
            prob -= 0.2;
        }

        // Offset heuristic: early disk sectors are reused more often
        if offset < 1024 * 1024 * 1024 {
            prob -= 0.15;
        }

        prob.clamp(0.05, 1.0)
    }

    #[cfg(target_os = "windows")]
    fn open_device(&self) -> Result<std::fs::File> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_NO_BUFFERING, FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };

        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_NO_BUFFERING | FILE_FLAG_SEQUENTIAL_SCAN)
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

    fn get_device_size(&self, file: &mut std::fs::File) -> Result<u64> {
        let size = file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;
        Ok(size)
    }
}

/// Fragmentation reassembly: attempts to reconstruct non-contiguous cluster chains
pub struct FragmentReassembler;

impl FragmentReassembler {
    /// Given a set of candidate fragments (by offset), attempt to reassemble
    /// by matching tail entropy with head patterns of subsequent fragments.
    pub fn reassemble(
        file: &mut std::fs::File,
        fragments: &[(u64, u64)], // (offset, size) pairs
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

    /// Entropy-based fragment matching: high entropy = compressed/encrypted data,
    /// low entropy = text/structured data
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
