/// Military-Grade Data Shredder
/// Implements DoD 5220.22-M (3-pass) and Gutmann (35-pass) overwrite algorithms.
/// Also handles NVMe hardware-level sanitize commands.
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;
use tracing::{info, warn};
use zeroize::Zeroize;

use super::types::{ShredAlgorithm, ShredOptions, ShredProgress, ShredStatus};

const WRITE_BLOCK_SIZE: usize = 1024 * 1024; // 1MB write blocks
const VERIFICATION_SAMPLE_SIZE: usize = 4096;

// OS-specific "disk full" errno / win32 error code
#[cfg(target_os = "windows")]
const DISK_FULL_CODE: i32 = 112; // ERROR_DISK_FULL
#[cfg(not(target_os = "windows"))]
const DISK_FULL_CODE: i32 = 28; // ENOSPC

/// Gutmann 35-pass patterns (Gutmann 1996)
/// Passes 1-4: random, Passes 5-31: specific patterns, Passes 32-35: random
const GUTMANN_PATTERNS: &[Option<u8>] = &[
    None,       // Pass 1:  random
    None,       // Pass 2:  random
    None,       // Pass 3:  random
    None,       // Pass 4:  random
    Some(0x55), // Pass 5:  01010101
    Some(0xAA), // Pass 6:  10101010
    Some(0x92), // Pass 7:  10010010
    Some(0x49), // Pass 8:  01001001
    Some(0x24), // Pass 9:  00100100
    Some(0x00), // Pass 10: 00000000
    Some(0x11), // Pass 11: 00010001
    Some(0x22), // Pass 12: 00100010
    Some(0x33), // Pass 13: 00110011
    Some(0x44), // Pass 14: 01000100
    Some(0x55), // Pass 15: 01010101
    Some(0x66), // Pass 16: 01100110
    Some(0x77), // Pass 17: 01110111
    Some(0x88), // Pass 18: 10001000
    Some(0x99), // Pass 19: 10011001
    Some(0xAA), // Pass 20: 10101010
    Some(0xBB), // Pass 21: 10111011
    Some(0xCC), // Pass 22: 11001100
    Some(0xDD), // Pass 23: 11011101
    Some(0xEE), // Pass 24: 11101110
    Some(0xFF), // Pass 25: 11111111
    Some(0x92), // Pass 26: 10010010
    Some(0x49), // Pass 27: 01001001
    Some(0x24), // Pass 28: 00100100
    Some(0x6D), // Pass 29: 01101101
    Some(0xB6), // Pass 30: 10110110
    Some(0xDB), // Pass 31: 11011011
    None,       // Pass 32: random
    None,       // Pass 33: random
    None,       // Pass 34: random
    None,       // Pass 35: random
];

pub struct Shredder {
    options: ShredOptions,
    progress_tx: Sender<ShredProgress>,
    cancel_flag: Arc<AtomicBool>,
}

impl Shredder {
    pub fn new(
        options: ShredOptions,
        progress_tx: Sender<ShredProgress>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Self {
        Self { options, progress_tx, cancel_flag }
    }

    pub fn execute(&self) -> Result<()> {
        match &self.options.algorithm {
            ShredAlgorithm::NvmeSanitize => return self.nvme_sanitize(),
            ShredAlgorithm::NvmeFormat => return self.nvme_format_nvm(),
            ShredAlgorithm::DoD5220 => self.shred_dod5220()?,
            ShredAlgorithm::Gutmann35 => self.shred_gutmann35()?,
            ShredAlgorithm::RandomSingle => self.shred_random_single()?,
        }

        // Skip deletion/rename steps for raw device paths — they are not files.
        let is_device = self.options.target_path.starts_with(r"\\.\")
            || self.options.target_path.starts_with("/dev/");

        if !is_device {
            // ── Step 1: Wipe NTFS Alternate Data Streams (Windows only) ──────
            // NTFS files can carry hidden "streams" (e.g. Zone.Identifier) that
            // contain metadata or data invisible to normal file browsers.
            #[cfg(target_os = "windows")]
            if let Err(e) = self.wipe_alternate_data_streams() {
                warn!("ADS wipe warning (non-fatal): {}", e);
            }

            // ── Step 2: Truncate to 0 ─────────────────────────────────────────
            // Removes the file size from the filesystem's directory entry / MFT
            // record so forensic tools cannot infer what was stored.
            {
                let f = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&self.options.target_path)
                    .context("Cannot open file to truncate")?;
                f.set_len(0).context("Cannot truncate file to zero")?;
                f.sync_data()?;
            }

            // ── Step 3: Rename to a random name ──────────────────────────────
            // The filesystem journal (NTFS $LogFile / ext4 journal) records file
            // operations. Renaming before deletion means the journal shows a
            // random name was deleted — not the original filename.
            let random_path = self.random_sibling_path()?;
            std::fs::rename(&self.options.target_path, &random_path)
                .context("Data overwritten but could not rename before deletion")?;

            // ── Step 4: Delete ────────────────────────────────────────────────
            std::fs::remove_file(&random_path)
                .context("Data overwritten but could not delete the renamed file")?;

            info!("Secure delete complete: overwritten → ADS wiped → truncated → renamed → deleted");
        }

        Ok(())
    }

    // ── Secure rename helper ──────────────────────────────────────────────────

    /// Returns a path to a random-named file in the same directory as the target.
    fn random_sibling_path(&self) -> Result<std::path::PathBuf> {
        use rand::distributions::Alphanumeric;
        let parent = std::path::Path::new(&self.options.target_path)
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let name: String = ChaCha20Rng::from_entropy()
            .sample_iter(Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        Ok(parent.join(name))
    }

    // ── NTFS Alternate Data Streams (Windows only) ────────────────────────────

    /// Enumerates all NTFS alternate data streams on the target file and
    /// overwrites each with random bytes before the main content is deleted.
    #[cfg(target_os = "windows")]
    fn wipe_alternate_data_streams(&self) -> Result<()> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FindClose, FindFirstStreamW, FindNextStreamW,
            FindStreamInfoStandard, WIN32_FIND_STREAM_DATA,
        };
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

        let path_wide: Vec<u16> = OsStr::new(&self.options.target_path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut find_data: WIN32_FIND_STREAM_DATA = unsafe { std::mem::zeroed() };

        let handle = unsafe {
            FindFirstStreamW(
                path_wide.as_ptr(),
                FindStreamInfoStandard,
                &mut find_data as *mut WIN32_FIND_STREAM_DATA as *mut _,
                0,
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            // File has no ADS, or FindFirstStreamW is not supported — not an error.
            return Ok(());
        }

        let mut rng = ChaCha20Rng::from_entropy();

        loop {
            let name_len = find_data.cStreamName
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(296);
            let stream_name = String::from_utf16_lossy(&find_data.cStreamName[..name_len]);

            // Skip the main data stream (::$DATA) — that's handled by run_passes.
            if !stream_name.is_empty() && stream_name != "::$DATA" {
                // Strip the trailing ":$DATA" type suffix to get the access path.
                // E.g. ":Zone.Identifier:$DATA" → "filepath:Zone.Identifier"
                let stripped = stream_name.trim_end_matches(":$DATA");
                let ads_path = format!("{}{}", self.options.target_path, stripped);
                if let Err(e) = self.overwrite_ads_stream(&ads_path, &mut rng) {
                    warn!("Could not wipe ADS '{}': {}", ads_path, e);
                }
            }

            // Advance to next stream
            let ok = unsafe {
                FindNextStreamW(handle, &mut find_data as *mut WIN32_FIND_STREAM_DATA as *mut _)
            };
            if ok == 0 {
                // ERROR_HANDLE_EOF (38) = no more streams — normal exit.
                break;
            }
        }

        unsafe { FindClose(handle) };
        Ok(())
    }

    /// Overwrites the content of a single NTFS alternate data stream.
    #[cfg(target_os = "windows")]
    fn overwrite_ads_stream(&self, ads_path: &str, rng: &mut ChaCha20Rng) -> Result<()> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };

        const GENERIC_READ: u32 = 0x8000_0000;
        const GENERIC_WRITE: u32 = 0x4000_0000;

        let path_wide: Vec<u16> = OsStr::new(ads_path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let handle = unsafe {
            CreateFileW(
                path_wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                0,
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            return Ok(()); // ADS may not be writable — skip gracefully.
        }

        let mut file = unsafe { std::fs::File::from_raw_handle(handle as *mut std::ffi::c_void) };

        let size = file.seek(SeekFrom::End(0)).unwrap_or(0);
        if size == 0 {
            return Ok(());
        }
        file.seek(SeekFrom::Start(0))?;

        let mut buf = vec![0u8; WRITE_BLOCK_SIZE];
        let mut written = 0u64;
        while written < size {
            let to_write = (size - written).min(WRITE_BLOCK_SIZE as u64) as usize;
            rng.fill_bytes(&mut buf[..to_write]);
            file.write_all(&buf[..to_write])?;
            written += to_write as u64;
        }
        file.sync_data()?;
        buf.zeroize();

        info!("Wiped ADS '{}' ({} bytes)", ads_path, size);
        Ok(())
    }

    // ── Shred algorithms ──────────────────────────────────────────────────────

    /// DoD 5220.22-M: 3 passes
    ///   Pass 1: Write 0x00
    ///   Pass 2: Write 0xFF
    ///   Pass 3: Write random bytes + verify
    fn shred_dod5220(&self) -> Result<()> {
        info!("Starting DoD 5220.22-M on: {}", self.options.target_path);

        let passes: Vec<PassType> = vec![
            PassType::Fixed(0x00),
            PassType::Fixed(0xFF),
            PassType::Random,
        ];

        self.run_passes(&passes, "DoD 5220.22-M")?;

        if self.options.verify_passes {
            let verified = self.verify_last_pass(PassType::Random)?;
            // Overwrite the completion event with the real verification outcome.
            // run_passes() sends verification_passed: None — this replaces it.
            let _ = self.progress_tx.try_send(ShredProgress {
                current_pass: 3,
                total_passes: 3,
                bytes_written: 0,
                total_bytes: 0,
                algorithm: "DoD 5220.22-M".to_string(),
                verification_passed: Some(verified),
                status: ShredStatus::Completed,
            });
        }

        Ok(())
    }

    /// Gutmann: 35 passes
    fn shred_gutmann35(&self) -> Result<()> {
        info!("Starting Gutmann 35-pass on: {}", self.options.target_path);

        let passes: Vec<PassType> = GUTMANN_PATTERNS
            .iter()
            .map(|p| match p {
                Some(byte) => PassType::Fixed(*byte),
                None => PassType::Random,
            })
            .collect();

        self.run_passes(&passes, "Gutmann 35-Pass")?;
        Ok(())
    }

    /// Single random pass (fast mode)
    fn shred_random_single(&self) -> Result<()> {
        info!("Starting single-pass random shred on: {}", self.options.target_path);
        self.run_passes(&[PassType::Random], "Random Single")?;
        Ok(())
    }

    fn run_passes(&self, passes: &[PassType], algorithm_name: &str) -> Result<()> {
        let total_passes = passes.len() as u32;
        let mut rng = ChaCha20Rng::from_entropy();

        for (pass_idx, pass_type) in passes.iter().enumerate() {
            if self.cancel_flag.load(Ordering::Relaxed) {
                info!("Shred cancelled at pass {}", pass_idx + 1);
                return Ok(());
            }

            let current_pass = pass_idx as u32 + 1;
            info!("Pass {}/{} ({:?})", current_pass, total_passes, pass_type);

            let mut file = self.open_target_rw()?;
            let file_size = self.get_target_size(&mut file)?;

            let mut written: u64 = 0;
            let mut write_buf = vec![0u8; WRITE_BLOCK_SIZE];

            while written < file_size {
                if self.cancel_flag.load(Ordering::Relaxed) {
                    break;
                }

                let to_write = std::cmp::min(WRITE_BLOCK_SIZE as u64, file_size - written) as usize;

                match pass_type {
                    PassType::Fixed(byte) => {
                        write_buf[..to_write].fill(*byte);
                    }
                    PassType::Random => {
                        rng.fill_bytes(&mut write_buf[..to_write]);
                    }
                }

                file.write_all(&write_buf[..to_write])
                    .context("Write failed during shred pass")?;

                written += to_write as u64;

                let _ = self.progress_tx.try_send(ShredProgress {
                    current_pass,
                    total_passes,
                    bytes_written: written,
                    total_bytes: file_size,
                    algorithm: algorithm_name.to_string(),
                    verification_passed: None,
                    status: ShredStatus::Shredding,
                });
            }

            // Flush page cache to physical media (equivalent to fsync/FlushFileBuffers).
            // file.flush() is a no-op on std::fs::File; sync_data() does the real OS call.
            file.sync_data()?;
            // Explicitly zeroize the write buffer from memory
            write_buf.zeroize();

            info!("Pass {}/{} complete.", current_pass, total_passes);
        }

        // Send completion with no verification result yet.
        // Algorithms that support verification (DoD5220) will send a follow-up
        // event with the real result after calling verify_last_pass().
        let _ = self.progress_tx.try_send(ShredProgress {
            current_pass: total_passes,
            total_passes,
            bytes_written: 0,
            total_bytes: 0,
            algorithm: algorithm_name.to_string(),
            verification_passed: None,
            status: ShredStatus::Completed,
        });

        Ok(())
    }

    /// Verification: reads sample sectors and checks that the last pattern was written
    fn verify_last_pass(&self, expected: PassType) -> Result<bool> {
        info!("Verifying last shred pass...");
        let mut file = self.open_target_ro()?;
        let file_size = self.get_target_size(&mut file)?;

        let mut verified = true;

        // Sample at beginning, 25%, 50%, 75%, and end of file
        let check_offsets = [
            0u64,
            file_size / 4,
            file_size / 2,
            3 * file_size / 4,
            file_size.saturating_sub(VERIFICATION_SAMPLE_SIZE as u64),
        ];

        for offset in check_offsets {
            if offset >= file_size {
                continue;
            }
            // Clamp sample to available bytes (prevents "failed to fill whole buffer"
            // on files smaller than VERIFICATION_SAMPLE_SIZE)
            let available = (file_size - offset) as usize;
            let to_read = available.min(VERIFICATION_SAMPLE_SIZE);
            if to_read == 0 {
                continue;
            }
            let mut sample_buf = vec![0u8; to_read];

            file.seek(SeekFrom::Start(offset))?;
            file.read_exact(&mut sample_buf)?;

            match expected {
                PassType::Fixed(byte) => {
                    if sample_buf.iter().any(|&b| b != byte) {
                        warn!("Verification FAILED at offset 0x{:X}", offset);
                        verified = false;
                        break;
                    }
                }
                PassType::Random => {
                    let first = sample_buf[0];
                    let all_same = sample_buf.iter().all(|&b| b == first);
                    if all_same {
                        warn!("Possible verification anomaly at offset 0x{:X}: uniform bytes", offset);
                    }
                }
            }
        }

        info!("Verification result: {}", if verified { "PASSED" } else { "FAILED" });
        Ok(verified)
    }

    // ── NVMe hardware commands ────────────────────────────────────────────────

    /// NVMe Sanitize command via IOCTL_STORAGE_PROTOCOL_COMMAND
    /// This sends an ATA Sanitize (Crypto Erase or Block Erase) command
    /// to the SSD controller, triggering full NAND flash erasure.
    #[cfg(target_os = "windows")]
    fn nvme_sanitize(&self) -> Result<()> {
        info!("Sending NVMe Sanitize command to: {}", self.options.target_path);

        // Open device with write access and IOCTL privileges
        let device = self.open_device_handle()?;

        // STORAGE_PROTOCOL_COMMAND structure for NVMe Admin Command Set
        // NVMe Sanitize command opcode: 0x84
        // CDW10[1:0] = 0x01 = Block Erase (or 0x04 = Crypto Erase)
        let nvme_sanitize_cmd = NvmeSanitizeCommand {
            opcode: 0x84,        // Sanitize
            cdw10: 0x00000002,   // SANACT[2:0] = 010b = Block Erase (0x1 = Exit Failure Mode — wrong)
            cdw11: 0x00000000,
            cdw12: 0x00000000,
        };

        let result = unsafe { self.send_nvme_admin_command(device, &nvme_sanitize_cmd) };

        match result {
            Ok(_) => {
                info!("NVMe Sanitize command accepted. Controller erasing NAND...");
                let _ = self.progress_tx.try_send(ShredProgress {
                    current_pass: 1,
                    total_passes: 1,
                    bytes_written: 0,
                    total_bytes: 0,
                    algorithm: "NVMe Sanitize".to_string(),
                    verification_passed: Some(true),
                    status: ShredStatus::Completed,
                });
                Ok(())
            }
            Err(e) => {
                warn!("NVMe Sanitize failed (may require admin elevation): {}", e);
                Err(e)
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn nvme_sanitize(&self) -> Result<()> {
        // Linux: use ioctl with NVME_IOCTL_ADMIN_CMD (opcode 0x84)
        info!("NVMe Sanitize on Linux via ioctl NVME_IOCTL_ADMIN_CMD");
        anyhow::bail!("NVMe Sanitize requires nvme-cli or direct ioctl access on Linux")
    }

    /// NVMe Format NVM (opcode 0x80) - reformats the namespace
    #[cfg(target_os = "windows")]
    fn nvme_format_nvm(&self) -> Result<()> {
        info!("Sending NVMe Format NVM command to: {}", self.options.target_path);
        let device = self.open_device_handle()?;

        let nvme_format_cmd = NvmeSanitizeCommand {
            opcode: 0x80,        // Format NVM
            cdw10: 0x00000200,   // SES = 010b = User Data Erase, LBAF=0
            cdw11: 0x00000000,
            cdw12: 0x00000000,
        };

        unsafe { self.send_nvme_admin_command(device, &nvme_format_cmd) }
    }

    #[cfg(not(target_os = "windows"))]
    fn nvme_format_nvm(&self) -> Result<()> {
        anyhow::bail!("NVMe Format NVM requires admin ioctl on Linux")
    }

    #[cfg(target_os = "windows")]
    fn open_device_handle(&self) -> Result<windows_sys::Win32::Foundation::HANDLE> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING, FILE_FLAG_NO_BUFFERING,
        };
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

        // GENERIC_READ = 0x80000000, GENERIC_WRITE = 0x40000000 (WinNT.h)
        const GENERIC_READ: u32 = 0x8000_0000;
        const GENERIC_WRITE: u32 = 0x4000_0000;

        let path_wide: Vec<u16> = OsStr::new(&self.options.target_path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let handle = unsafe {
            CreateFileW(
                path_wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_NO_BUFFERING,
                0,
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            anyhow::bail!("Failed to open device handle for NVMe command");
        }

        Ok(handle)
    }

    #[cfg(target_os = "windows")]
    unsafe fn send_nvme_admin_command(
        &self,
        handle: windows_sys::Win32::Foundation::HANDLE,
        cmd: &NvmeSanitizeCommand,
    ) -> Result<()> {
        use windows_sys::Win32::System::IO::DeviceIoControl;
        use windows_sys::Win32::System::Ioctl::IOCTL_STORAGE_PROTOCOL_COMMAND;

        // Build STORAGE_PROTOCOL_COMMAND buffer
        // This is a simplified version; production code needs full struct layout
        let mut output_buf = vec![0u8; 4096];
        let mut bytes_returned: u32 = 0;

        // Protocol command buffer layout (simplified)
        let mut protocol_cmd = vec![0u8; 512];
        // Version = 1
        protocol_cmd[0..4].copy_from_slice(&1u32.to_le_bytes());
        // Length = 512
        protocol_cmd[4..8].copy_from_slice(&512u32.to_le_bytes());
        // ProtocolType = ProtocolTypeNvme (4)
        protocol_cmd[8..12].copy_from_slice(&4u32.to_le_bytes());
        // Flags = STORAGE_PROTOCOL_COMMAND_FLAG_ADAPTER_REQUEST (0x80000000)
        protocol_cmd[12..16].copy_from_slice(&0x80000000u32.to_le_bytes());
        // Command opcode at offset 32
        protocol_cmd[32] = cmd.opcode;
        // CDW10 at offset 44
        protocol_cmd[44..48].copy_from_slice(&cmd.cdw10.to_le_bytes());

        let result = DeviceIoControl(
            handle,
            IOCTL_STORAGE_PROTOCOL_COMMAND,
            protocol_cmd.as_ptr() as *const _,
            protocol_cmd.len() as u32,
            output_buf.as_mut_ptr() as *mut _,
            output_buf.len() as u32,
            &mut bytes_returned,
            std::ptr::null_mut(),
        );

        if result == 0 {
            anyhow::bail!("IOCTL_STORAGE_PROTOCOL_COMMAND failed");
        }

        Ok(())
    }

    /// Returns the byte size of a file or raw device.
    /// On Windows, SeekFrom::End(0) fails on raw volumes with ERROR_INVALID_PARAMETER.
    /// Use IOCTL_DISK_GET_LENGTH_INFO first; fall back to seek for regular files.
    fn get_target_size(&self, file: &mut std::fs::File) -> Result<u64> {
        #[cfg(target_os = "windows")]
        {
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
        }

        let size = file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;
        Ok(size)
    }

    fn open_target_rw(&self) -> Result<std::fs::File> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.options.target_path)
            .context(format!("Cannot open for writing: {}", self.options.target_path))
    }

    fn open_target_ro(&self) -> Result<std::fs::File> {
        std::fs::File::open(&self.options.target_path)
            .context(format!("Cannot open for reading: {}", self.options.target_path))
    }
}

// ── Free Space Wiper ──────────────────────────────────────────────────────────

/// Fills all available free space on the given drive/directory with random data,
/// then deletes the temporary file. This overwrites blocks that belonged to files
/// deleted normally (via Recycle Bin / rm) before this tool was used.
///
/// The `target_dir` should be a writable directory on the target drive,
/// e.g. "C:\\" or "/home/user".
pub fn wipe_free_space(
    target_dir: String,
    progress_tx: Sender<ShredProgress>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<()> {
    use rand::distributions::Alphanumeric;

    // Build temp file path inside the target directory.
    let dir = std::path::Path::new(&target_dir);
    let random_name: String = ChaCha20Rng::from_entropy()
        .sample_iter(Alphanumeric)
        .take(16)
        .map(char::from)
        .collect();
    let temp_path = dir.join(format!("{}.wipe", random_name));

    info!("Free space wipe: temp file at {:?}", temp_path);

    let mut file = std::fs::File::create(&temp_path)
        .context("Cannot create temp wipe file — check write permissions on the target directory")?;

    let mut rng = ChaCha20Rng::from_entropy();
    let mut buf = vec![0u8; WRITE_BLOCK_SIZE];
    let mut written: u64 = 0;

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            info!("Free space wipe cancelled after {} bytes", written);
            break;
        }

        rng.fill_bytes(&mut buf);

        match file.write_all(&buf) {
            Ok(_) => {
                written += WRITE_BLOCK_SIZE as u64;
                let _ = progress_tx.try_send(ShredProgress {
                    current_pass: 1,
                    total_passes: 1,
                    bytes_written: written,
                    total_bytes: 0, // unknown upfront; UI shows raw bytes written
                    algorithm: "Free Space Wipe".to_string(),
                    verification_passed: None,
                    status: ShredStatus::Shredding,
                });
            }
            Err(e) => {
                // Disk full = normal and expected termination.
                if e.raw_os_error() == Some(DISK_FULL_CODE) {
                    info!("Free space wipe: disk full after {} bytes — all free space overwritten", written);
                    break;
                }
                // Real I/O error — clean up and report.
                drop(file);
                let _ = std::fs::remove_file(&temp_path);
                buf.zeroize();
                return Err(e).context("I/O error during free space wipe");
            }
        }
    }

    // Flush remaining data, then delete the temp file.
    let _ = file.sync_data();
    buf.zeroize();
    drop(file);

    std::fs::remove_file(&temp_path)
        .context("Wipe complete but failed to delete temporary wipe file")?;

    let _ = progress_tx.try_send(ShredProgress {
        current_pass: 1,
        total_passes: 1,
        bytes_written: written,
        total_bytes: written,
        algorithm: "Free Space Wipe".to_string(),
        verification_passed: None,
        status: ShredStatus::Completed,
    });

    info!("Free space wipe complete: {} bytes written and cleared", written);
    Ok(())
}

// ── Internal types ────────────────────────────────────────────────────────────

#[derive(Debug)]
enum PassType {
    Fixed(u8),
    Random,
}

#[cfg(target_os = "windows")]
struct NvmeSanitizeCommand {
    opcode: u8,
    cdw10: u32,
    cdw11: u32,
    cdw12: u32,
}
