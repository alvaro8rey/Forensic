/// MFT (Master File Table) Parser for NTFS
/// Identifies deleted file records (marked 0x00) that SSD GC hasn't cleaned yet.
use std::io::{Read, Seek, SeekFrom};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

const MFT_SIGNATURE: &[u8; 4] = b"FILE";
const MFT_RECORD_SIZE: u64 = 1024; // Standard MFT record size
const NTFS_BOOT_SECTOR_OFFSET: u64 = 0;
const SECTOR_SIZE: u64 = 512;

/// NTFS Boot Sector (BPB - BIOS Parameter Block)
#[derive(Debug)]
pub struct NtfsBpb {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub mft_cluster_number: u64,
    pub mft_mirror_cluster: u64,
    pub clusters_per_mft_record: i8,
    pub volume_serial_number: u64,
    pub total_sectors: u64,
}

/// MFT Record Header (simplified)
#[derive(Debug)]
pub struct MftRecordHeader {
    pub signature: [u8; 4],        // "FILE" or 0x00 for deleted
    pub sequence_number: u16,
    pub hard_link_count: u16,
    pub flags: u16,                // 0x0001 = in use, 0x0002 = directory
    pub bytes_allocated: u32,
    pub base_file_record: u64,
    pub next_attr_id: u16,
    pub record_number: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletedMftEntry {
    pub record_number: u64,
    pub file_name: Option<String>,
    pub file_size: u64,
    pub created_time: u64,
    pub modified_time: u64,
    pub data_run_offset: u64,    // LCN (Logical Cluster Number) where data lived
    pub is_recoverable: bool,
    pub file_type_hint: String,
}

pub struct MftParser {
    device_path: String,
}

impl MftParser {
    pub fn new(device_path: String) -> Self {
        Self { device_path }
    }

    pub fn parse_deleted_entries(&self) -> Result<Vec<DeletedMftEntry>> {
        let mut file = self.open_device()?;
        let bpb = self.read_ntfs_bpb(&mut file)?;

        info!("NTFS volume detected. MFT at cluster {}", bpb.mft_cluster_number);

        let bytes_per_cluster = bpb.bytes_per_sector as u64 * bpb.sectors_per_cluster as u64;
        let mft_offset = bpb.mft_cluster_number * bytes_per_cluster;

        info!("MFT physical offset: 0x{:016X}", mft_offset);

        let mut deleted_entries = Vec::new();
        let mut record_offset = mft_offset;
        let mut record_num: u64 = 0;

        // Read MFT records sequentially
        // The first 16 records are system files ($MFT, $MFTMirr, $LogFile, etc.)
        // User files start at record 24 onwards
        loop {
            file.seek(SeekFrom::Start(record_offset))
                .context("Failed to seek to MFT record")?;

            let mut record_buf = vec![0u8; MFT_RECORD_SIZE as usize];
            match file.read_exact(&mut record_buf) {
                Ok(_) => {}
                Err(_) => break,
            }

            // Check if this is a deleted MFT record
            // Deleted records have signature overwritten with 0x00 or "BAAD"
            let sig = &record_buf[0..4];

            if sig == MFT_SIGNATURE {
                // Active record - check flags
                let flags = u16::from_le_bytes([record_buf[22], record_buf[23]]);
                if flags & 0x0001 == 0 {
                    // Not in-use = deleted
                    if let Some(entry) = self.parse_deleted_record(&record_buf, record_num) {
                        debug!("Found deleted MFT record #{}: {:?}", record_num, entry.file_name);
                        deleted_entries.push(entry);
                    }
                }
            } else if sig == [0x00, 0x00, 0x00, 0x00] {
                // Zeroed-out record (SSD TRIM applied or secure delete)
                // Still attempt partial recovery from residual data
                if let Some(partial) = self.attempt_partial_recovery(&record_buf, record_num) {
                    deleted_entries.push(partial);
                }
            }

            record_offset += MFT_RECORD_SIZE;
            record_num += 1;

            // Safety limit: scan up to 10M MFT records
            if record_num > 10_000_000 {
                break;
            }
        }

        info!("MFT scan complete. Found {} deleted entries.", deleted_entries.len());
        Ok(deleted_entries)
    }

    fn read_ntfs_bpb(&self, file: &mut std::fs::File) -> Result<NtfsBpb> {
        file.seek(SeekFrom::Start(NTFS_BOOT_SECTOR_OFFSET))?;
        let mut sector = vec![0u8; 512];
        file.read_exact(&mut sector)?;

        // Verify NTFS OEM ID at offset 3
        let oem_id = &sector[3..11];
        if oem_id != b"NTFS    " {
            anyhow::bail!("Not an NTFS volume (OEM ID: {:?})", std::str::from_utf8(oem_id));
        }

        let bytes_per_sector = u16::from_le_bytes([sector[11], sector[12]]);
        let sectors_per_cluster = sector[13];
        let total_sectors = u64::from_le_bytes([
            sector[40], sector[41], sector[42], sector[43],
            sector[44], sector[45], sector[46], sector[47],
        ]);
        let mft_cluster = u64::from_le_bytes([
            sector[48], sector[49], sector[50], sector[51],
            sector[52], sector[53], sector[54], sector[55],
        ]);
        let mft_mirror = u64::from_le_bytes([
            sector[56], sector[57], sector[58], sector[59],
            sector[60], sector[61], sector[62], sector[63],
        ]);
        let clusters_per_mft = sector[64] as i8;
        let volume_serial = u64::from_le_bytes([
            sector[72], sector[73], sector[74], sector[75],
            sector[76], sector[77], sector[78], sector[79],
        ]);

        Ok(NtfsBpb {
            bytes_per_sector,
            sectors_per_cluster,
            mft_cluster_number: mft_cluster,
            mft_mirror_cluster: mft_mirror,
            clusters_per_mft_record: clusters_per_mft,
            volume_serial_number: volume_serial,
            total_sectors,
        })
    }

    fn parse_deleted_record(&self, record: &[u8], record_num: u64) -> Option<DeletedMftEntry> {
        if record.len() < 48 {
            return None;
        }

        // Parse attribute list starting at first_attr_offset
        let first_attr_off = u16::from_le_bytes([record[20], record[21]]) as usize;
        if first_attr_off >= record.len() {
            return None;
        }

        let mut file_name: Option<String> = None;
        let mut file_size: u64 = 0;
        let mut data_run_offset: u64 = 0;
        let mut created_time: u64 = 0;
        let mut modified_time: u64 = 0;

        let mut attr_off = first_attr_off;

        // Walk attributes
        while attr_off + 4 < record.len() {
            let attr_type = u32::from_le_bytes([
                record[attr_off],
                record[attr_off + 1],
                record[attr_off + 2],
                record[attr_off + 3],
            ]);

            if attr_type == 0xFFFFFFFF {
                break; // End marker
            }

            let attr_len = if attr_off + 8 <= record.len() {
                u32::from_le_bytes([
                    record[attr_off + 4],
                    record[attr_off + 5],
                    record[attr_off + 6],
                    record[attr_off + 7],
                ]) as usize
            } else {
                break;
            };

            if attr_len == 0 || attr_off + attr_len > record.len() {
                break;
            }

            match attr_type {
                // $STANDARD_INFORMATION (0x10)
                0x10 => {
                    if attr_off + 24 + 16 <= record.len() {
                        let val_off = record[attr_off + 20] as usize;
                        let base = attr_off + val_off;
                        if base + 16 <= record.len() {
                            created_time = u64::from_le_bytes(record[base..base+8].try_into().unwrap_or([0;8]));
                            modified_time = u64::from_le_bytes(record[base+8..base+16].try_into().unwrap_or([0;8]));
                        }
                    }
                }
                // $FILE_NAME (0x30)
                0x30 => {
                    if let Some(name) = self.parse_filename_attr(&record[attr_off..attr_off + attr_len]) {
                        file_name = Some(name);
                    }
                }
                // $DATA (0x80)
                0x80 => {
                    if attr_off + 8 < record.len() {
                        let non_resident = record[attr_off + 8];
                        if non_resident == 1 {
                            // Non-resident: parse data runs
                            if attr_off + 32 + 8 <= record.len() {
                                let real_size_off = attr_off + 48;
                                if real_size_off + 8 <= record.len() {
                                    file_size = u64::from_le_bytes(
                                        record[real_size_off..real_size_off+8].try_into().unwrap_or([0;8])
                                    );
                                }
                                let run_off = attr_off + record[attr_off + 32] as usize;
                                if run_off < record.len() {
                                    data_run_offset = run_off as u64;
                                }
                            }
                        } else {
                            // Resident data: size in content length
                            if attr_off + 16 + 4 <= record.len() {
                                file_size = u32::from_le_bytes([
                                    record[attr_off + 16], record[attr_off + 17],
                                    record[attr_off + 18], record[attr_off + 19],
                                ]) as u64;
                            }
                        }
                    }
                }
                _ => {}
            }

            attr_off += attr_len;
        }

        // Determine file type from extension
        let file_type_hint = file_name.as_deref()
            .and_then(|n| n.rsplit('.').next())
            .unwrap_or("unknown")
            .to_uppercase();

        Some(DeletedMftEntry {
            record_number: record_num,
            file_name,
            file_size,
            created_time,
            modified_time,
            data_run_offset,
            is_recoverable: data_run_offset > 0,
            file_type_hint,
        })
    }

    fn parse_filename_attr(&self, attr: &[u8]) -> Option<String> {
        if attr.len() < 8 {
            return None;
        }
        // Value offset at bytes 20-21 (resident attribute layout)
        let val_off = if attr.len() > 21 { attr[20] as usize } else { return None; };
        // $FILE_NAME attribute value: 66 bytes header + variable Unicode name
        let fn_base = val_off + 66;
        if fn_base >= attr.len() {
            return None;
        }
        let name_len = *attr.get(val_off + 64)? as usize;
        let name_bytes_len = name_len * 2;
        if fn_base + name_bytes_len > attr.len() {
            return None;
        }

        let name_utf16: Vec<u16> = attr[fn_base..fn_base + name_bytes_len]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();

        String::from_utf16(&name_utf16).ok()
    }

    fn attempt_partial_recovery(&self, record: &[u8], record_num: u64) -> Option<DeletedMftEntry> {
        // Check if any non-zero bytes remain (GC incomplete)
        let non_zero = record.iter().any(|&b| b != 0);
        if !non_zero {
            return None;
        }

        Some(DeletedMftEntry {
            record_number: record_num,
            file_name: None,
            file_size: 0,
            created_time: 0,
            modified_time: 0,
            data_run_offset: 0,
            is_recoverable: false,
            file_type_hint: "PARTIAL".to_string(),
        })
    }

    #[cfg(target_os = "windows")]
    fn open_device(&self) -> Result<std::fs::File> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_NO_BUFFERING, FILE_SHARE_READ, FILE_SHARE_WRITE};

        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_NO_BUFFERING)
            .open(&self.device_path)
            .context("Failed to open device for MFT parsing")
    }

    #[cfg(not(target_os = "windows"))]
    fn open_device(&self) -> Result<std::fs::File> {
        std::fs::OpenOptions::new()
            .read(true)
            .open(&self.device_path)
            .context("Failed to open device for MFT parsing")
    }
}

/// SSD Over-Provisioning Space Scanner
/// Attempts to read beyond reported LBA range to access OP space
/// Note: this requires vendor-specific commands; this implementation
/// uses a best-effort approach via sequential reads past the visible range.
pub struct OverProvisioningScanner;

impl OverProvisioningScanner {
    pub fn scan_op_space(device_path: &str) -> Result<Vec<u8>> {
        warn!("OP space scanning is vendor-specific. Results may be empty.");

        // On most SSDs, the controller maps OP to spare area not accessible via standard LBA
        // Best approach: use ATA SMART READ DATA to get raw NAND statistics
        // then use vendor-specific NVMe log pages (0xC0-0xFF range)

        // For simulation, we attempt reads at very high LBA addresses
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(device_path)
            .context("Cannot open device for OP scan")?;

        let mut file = file;
        let visible_end = file.seek(SeekFrom::End(0))?;

        // Attempt read 7% beyond reported size (typical OP ratio)
        let op_start = (visible_end as f64 * 1.0) as u64;
        let op_probe_size = (visible_end as f64 * 0.07) as u64;

        file.seek(SeekFrom::Start(op_start))?;
        let mut buf = vec![0u8; op_probe_size.min(1024 * 1024) as usize];

        match file.read(&mut buf) {
            Ok(n) => {
                info!("OP space probe read {} bytes at 0x{:X}", n, op_start);
                Ok(buf[..n].to_vec())
            }
            Err(e) => {
                warn!("OP space not accessible (expected on most SSDs): {}", e);
                Ok(vec![])
            }
        }
    }
}
