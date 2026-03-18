// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod modules;

use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::io::{Read, Seek, SeekFrom, Write};
use crossbeam_channel::unbounded;
use tauri::{State, Window};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use modules::{
    carver::FileCarver,
    fat::FatCarver,
    mft::{DeletedMftEntry, MftParser},
    shredder::Shredder,
    smart::SmartReader,
    types::{DiskInfo, FileType, RecoveredFile, ScanProgress, ScanStatus, ShredAlgorithm, ShredOptions, ShredProgress, ValidationStatus},
};

// ─── Global App State ────────────────────────────────────────────────────────

struct AppState {
    scan_cancel: Arc<AtomicBool>,
    shred_cancel: Arc<AtomicBool>,
    scan_results: Arc<Mutex<Vec<RecoveredFile>>>,
    mft_results: Arc<Mutex<Vec<DeletedMftEntry>>>,
    /// Device path used for the last scan (needed to extract bytes during recovery)
    scan_device: Arc<Mutex<String>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            scan_cancel: Arc::new(AtomicBool::new(false)),
            shred_cancel: Arc::new(AtomicBool::new(false)),
            scan_results: Arc::new(Mutex::new(Vec::new())),
            mft_results: Arc::new(Mutex::new(Vec::new())),
            scan_device: Arc::new(Mutex::new(String::new())),
        }
    }
}

// ─── Tauri Commands ───────────────────────────────────────────────────────────

/// List all available disks with S.M.A.R.T. health info
#[tauri::command]
async fn list_disks(_state: State<'_, AppState>) -> Result<Vec<DiskInfo>, String> {
    info!("Command: list_disks");
    let disks = SmartReader::list_disks();
    Ok(disks)
}

/// Maps a profile string + optional custom type list to a set of allowed FileTypes.
/// Returns `None` for "full" (scan everything) or `Some(vec)` to restrict.
fn resolve_allowed_types(profile: &str, custom_types: &[String]) -> Option<Vec<FileType>> {
    match profile {
        "fast" => Some(vec![
            FileType::JPEG, FileType::PNG, FileType::GIF, FileType::TIFF, FileType::BMP,
            FileType::PDF, FileType::DOCX, FileType::XLSX, FileType::PPTX, FileType::DOC,
            FileType::TXT,
        ]),
        "custom" if !custom_types.is_empty() => Some(
            custom_types.iter().filter_map(|s| match s.as_str() {
                "JPEG"   => Some(FileType::JPEG),
                "PNG"    => Some(FileType::PNG),
                "GIF"    => Some(FileType::GIF),
                "TIFF"   => Some(FileType::TIFF),
                "BMP"    => Some(FileType::BMP),
                "PDF"    => Some(FileType::PDF),
                "DOCX"   => Some(FileType::DOCX),
                "XLSX"   => Some(FileType::XLSX),
                "PPTX"   => Some(FileType::PPTX),
                "DOC"    => Some(FileType::DOC),
                "TXT"    => Some(FileType::TXT),
                "ZIP"    => Some(FileType::ZIP),
                "RAR"    => Some(FileType::RAR),
                "SevenZ" => Some(FileType::SevenZ),
                "EXE"    => Some(FileType::EXE),
                "MP4"    => Some(FileType::MP4),
                "AVI"    => Some(FileType::AVI),
                "MKV"    => Some(FileType::MKV),
                "MP3"    => Some(FileType::MP3),
                "WAV"    => Some(FileType::WAV),
                "FLAC"   => Some(FileType::FLAC),
                "SQLite" => Some(FileType::SQLite),
                _        => None,
            }).collect()
        ),
        _ => None, // "full" — no filter
    }
}

/// Start a file-carving scan on a device
/// Emits `scan-progress` events to the frontend in real-time
#[tauri::command]
async fn start_scan(
    device_path: String,
    scan_profile: String,
    custom_types: Vec<String>,
    window: Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Command: start_scan on {} [profile={}]", device_path, scan_profile);

    // Reset cancel flag and store device path for later recovery
    state.scan_cancel.store(false, Ordering::SeqCst);
    *state.scan_device.lock().unwrap() = device_path.clone();

    let allowed_types = resolve_allowed_types(&scan_profile, &custom_types);
    info!("Scan profile '{}': {} type(s) active",
        scan_profile,
        allowed_types.as_ref().map(|v| v.len()).unwrap_or(0).max(99) // "all" when None
    );

    let (tx, rx) = unbounded::<ScanProgress>();
    let cancel = Arc::clone(&state.scan_cancel);
    let results_store = Arc::clone(&state.scan_results);
    let win_clone = window.clone();
    let path_clone = device_path.clone();

    // Spawn scan thread (blocking I/O)
    thread::spawn(move || {
        let mut all_results: Vec<RecoveredFile> = Vec::new();

        // ── Phase 1: FAT32/exFAT directory carving (fast) ────────────────
        let fat_carver = FatCarver::new(path_clone.clone());
        match fat_carver.scan() {
            Ok(mut fat_files) => {
                // When a profile is active, filter FAT results to the same allowed types
                if let Some(ref allowed) = allowed_types {
                    fat_files.retain(|f| allowed.contains(&f.file_type));
                }
                info!("FAT directory carving: {} deleted entries (after filter)", fat_files.len());
                all_results.append(&mut fat_files);
                // Emit an early progress snapshot so the UI shows FAT results immediately
                let _ = win_clone.emit("scan-progress", ScanProgress {
                    bytes_scanned: 0,
                    total_bytes: 1,
                    current_offset_hex: "0x0000000000000000".to_string(),
                    files_found: all_results.len() as u32,
                    scan_speed_mb: 0.0,
                    elapsed_seconds: 0,
                    status: ScanStatus::Scanning,
                });
            }
            Err(e) => {
                info!("FAT carving skipped ({})", e);
            }
        }

        // ── Phase 2: Signature-based carving (slow, emits progress) ──────
        let carver = FileCarver::new(path_clone, tx, cancel, allowed_types);
        match carver.scan() {
            Ok(sig_files) => {
                // Merge: FAT entries take priority; skip sig entries whose
                // (type, sector-aligned offset) already appear in FAT results.
                let fat_keys: std::collections::HashSet<String> = all_results
                    .iter()
                    .map(|f| format!("{:?}:{}", f.file_type, f.offset_start / 512 * 512))
                    .collect();

                for f in sig_files {
                    let key = format!("{:?}:{}", f.file_type, f.offset_start / 512 * 512);
                    if !fat_keys.contains(&key) {
                        all_results.push(f);
                    }
                }

                // Re-assign sequential IDs
                for (i, f) in all_results.iter_mut().enumerate() {
                    f.id = i as u64;
                }

                info!("Total files after merge: {}", all_results.len());
                *results_store.lock().unwrap() = all_results.clone();
                let _ = win_clone.emit("scan-complete", all_results);
            }
            Err(e) => {
                error!("Scan error: {}", e);
                let _ = win_clone.emit("scan-error", e.to_string());
            }
        }
    });

    // Progress relay thread
    let win_progress = window.clone();
    thread::spawn(move || {
        for progress in rx {
            let _ = win_progress.emit("scan-progress", &progress);
        }
    });

    Ok(())
}

/// Cancel an ongoing scan
#[tauri::command]
async fn cancel_scan(state: State<'_, AppState>) -> Result<(), String> {
    info!("Command: cancel_scan");
    state.scan_cancel.store(true, Ordering::SeqCst);
    Ok(())
}

/// Retrieve cached scan results
#[tauri::command]
async fn get_scan_results(state: State<'_, AppState>) -> Result<Vec<RecoveredFile>, String> {
    Ok(state.scan_results.lock().unwrap().clone())
}

/// Structured result returned by recover_file.
#[derive(serde::Serialize)]
struct RecoverResult {
    key: String,
    file_type: String,
    kb: usize,
    path: String,
    validation: ValidationStatus,
}

/// Per-file result for batch recovery.
#[derive(serde::Serialize)]
struct BatchRecoverResult {
    file_id: u64,
    success: bool,
    path: String,
    error: Option<String>,
    validation: Option<ValidationStatus>,
}

/// Recover (extract) a specific file from disk to a destination.
/// Reads raw bytes at the recorded offset from the scanned device.
#[tauri::command]
async fn recover_file(
    file_id: u64,
    destination_path: String,
    state: State<'_, AppState>,
) -> Result<RecoverResult, String> {
    info!("Command: recover_file id={} dest={}", file_id, destination_path);

    let (offset_start, size_bytes, type_str) = {
        let results = state.scan_results.lock().unwrap();
        let f = results
            .iter()
            .find(|f| f.id == file_id)
            .ok_or_else(|| format!("File ID {} not found in scan results", file_id))?;
        (f.offset_start, f.size_bytes, format!("{:?}", f.file_type))
    };

    let device_path = state.scan_device.lock().unwrap().clone();
    if device_path.is_empty() {
        return Err("No scan device recorded — run a scan first.".to_string());
    }

    // Cap read size: never allocate more than 500 MB at once
    let read_size = size_bytes.min(500 * 1024 * 1024) as usize;
    if read_size == 0 {
        return Err(format!("File ID {} has zero size — nothing to recover", file_id));
    }

    // Open the source device with shared read access
    let mut src = open_device_ro(&device_path)
        .map_err(|e| format!("Cannot open device '{}': {}", device_path, e))?;

    src.seek(SeekFrom::Start(offset_start))
        .map_err(|e| format!("Seek failed at 0x{:X}: {}", offset_start, e))?;

    // Robust read: never fail just because size_bytes was an estimate
    let data = read_bytes_robust(&mut src, read_size)
        .map_err(|e| format!("Read failed at 0x{:X}: {}", offset_start, e))?;

    // Write recovered bytes to the user-chosen destination
    let mut dst = std::fs::File::create(&destination_path)
        .map_err(|e| format!("Cannot create output file '{}': {}", destination_path, e))?;
    dst.write_all(&data)
        .map_err(|e| format!("Write failed: {}", e))?;

    let kb = data.len() / 1024;
    let validation = validate_bytes(&data, &type_str);
    info!("Recovered {} ({} KB) from 0x{:X} → {} [valid={}]",
        type_str, kb, offset_start, destination_path, validation.is_valid);
    Ok(RecoverResult {
        key: "recovery.recoveredMsg".to_string(),
        file_type: type_str,
        kb,
        path: destination_path,
        validation,
    })
}

/// Recover multiple files at once into a destination folder.
#[tauri::command]
async fn recover_batch(
    file_ids: Vec<u64>,
    destination_folder: String,
    state: State<'_, AppState>,
) -> Result<Vec<BatchRecoverResult>, String> {
    info!("Command: recover_batch {} files → {}", file_ids.len(), destination_folder);

    let entries: Vec<(u64, u64, u64, String, String)> = {
        let results = state.scan_results.lock().unwrap();
        file_ids.iter().filter_map(|&id| {
            results.iter().find(|f| f.id == id).map(|f| {
                let fname = f.original_name.clone()
                    .unwrap_or_else(|| {
                        let ext = crate::modules::fat::ext_to_filetype(&format!(".{}", f.file_type))
                            .to_string().to_lowercase();
                        format!("recovered_{}_{}.{}", f.file_type, f.id, ext)
                    });
                (f.id, f.offset_start, f.size_bytes, format!("{}", f.file_type), fname)
            })
        }).collect()
    };

    let device_path = state.scan_device.lock().unwrap().clone();
    if device_path.is_empty() {
        return Err("No scan device recorded — run a scan first.".to_string());
    }

    let mut src = open_device_ro(&device_path)
        .map_err(|e| format!("Cannot open device: {}", e))?;

    let mut batch_results = Vec::new();

    for (id, offset_start, size_bytes, type_str, filename) in entries {
        let read_size = size_bytes.min(500 * 1024 * 1024) as usize;
        let dest_path = format!("{}/{}", destination_folder.trim_end_matches(['/', '\\']), filename);

        let result = (|| -> Result<BatchRecoverResult, String> {
            src.seek(SeekFrom::Start(offset_start))
                .map_err(|e| format!("Seek failed: {}", e))?;
            let data = read_bytes_robust(&mut src, read_size)
                .map_err(|e| format!("Read failed: {}", e))?;
            let validation = validate_bytes(&data, &type_str);
            let mut dst = std::fs::File::create(&dest_path)
                .map_err(|e| format!("Cannot create file: {}", e))?;
            dst.write_all(&data).map_err(|e| format!("Write failed: {}", e))?;
            Ok(BatchRecoverResult {
                file_id: id,
                success: true,
                path: dest_path.clone(),
                error: None,
                validation: Some(validation),
            })
        })();

        batch_results.push(result.unwrap_or_else(|e| BatchRecoverResult {
            file_id: id,
            success: false,
            path: dest_path,
            error: Some(e),
            validation: None,
        }));
    }

    Ok(batch_results)
}

/// Pack selected recovered files into a single ZIP archive.
#[tauri::command]
async fn export_recovered_zip(
    file_ids: Vec<u64>,
    zip_path: String,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    info!("Command: export_recovered_zip {} files → {}", file_ids.len(), zip_path);

    let entries: Vec<(u64, u64, u64, String)> = {
        let results = state.scan_results.lock().unwrap();
        file_ids.iter().filter_map(|&id| {
            results.iter().find(|f| f.id == id).map(|f| {
                let ext = format!("{}", f.file_type).to_lowercase();
                let fname = f.original_name.clone()
                    .unwrap_or_else(|| format!("recovered_{}_{}.{}", f.file_type, f.id, ext));
                (f.id, f.offset_start, f.size_bytes, fname)
            })
        }).collect()
    };

    let device_path = state.scan_device.lock().unwrap().clone();
    if device_path.is_empty() {
        return Err("No scan device recorded.".to_string());
    }

    let zip_file = std::fs::File::create(&zip_path)
        .map_err(|e| format!("Cannot create ZIP: {}", e))?;
    let mut zip = zip::ZipWriter::new(zip_file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut src = open_device_ro(&device_path)
        .map_err(|e| format!("Cannot open device: {}", e))?;

    let mut count = 0usize;
    for (_id, offset_start, size_bytes, filename) in entries {
        let read_size = size_bytes.min(500 * 1024 * 1024) as usize;
        if src.seek(SeekFrom::Start(offset_start)).is_err() { continue; }
        let data = match read_bytes_robust(&mut src, read_size) {
            Ok(d) => d,
            Err(_) => continue,
        };

        // Ensure unique filenames inside the ZIP
        let entry_name = if count == 0 { filename.clone() }
            else {
                let dot = filename.rfind('.').unwrap_or(filename.len());
                format!("{}_{}{}", &filename[..dot], count, &filename[dot..])
            };

        if zip.start_file(&entry_name, options).is_ok() {
            let _ = zip.write_all(&data);
            count += 1;
        }
    }

    zip.finish().map_err(|e| format!("ZIP finalize failed: {}", e))?;
    info!("ZIP export: {} files → {}", count, zip_path);
    Ok(count)
}

/// Preview a file: read up to 5 MB and return as base64 (for images/text inline preview).
#[tauri::command]
async fn preview_file(
    file_id: u64,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let (offset_start, size_bytes, type_str) = {
        let results = state.scan_results.lock().unwrap();
        let f = results
            .iter()
            .find(|f| f.id == file_id)
            .ok_or_else(|| format!("File ID {} not found", file_id))?;
        (f.offset_start, f.size_bytes, format!("{}", f.file_type))
    };

    let device_path = state.scan_device.lock().unwrap().clone();
    if device_path.is_empty() {
        return Err("No scan device — run a scan first.".to_string());
    }

    let read_size = size_bytes.min(5 * 1024 * 1024) as usize; // cap at 5 MB
    if read_size == 0 {
        return Err("File has zero size".to_string());
    }
    let mut src = open_device_ro(&device_path)
        .map_err(|e| format!("Cannot open device: {}", e))?;
    src.seek(SeekFrom::Start(offset_start))
        .map_err(|e| format!("Seek failed: {}", e))?;

    // Robust read — don't fail on partial data
    let data = read_bytes_robust(&mut src, read_size)
        .map_err(|e| format!("Read failed: {}", e))?;

    // Return "mime:base64data" so the frontend knows the content type
    let mime = match type_str.as_str() {
        "JPEG"   => "image/jpeg",
        "PNG"    => "image/png",
        "GIF"    => "image/gif",
        "BMP"    => "image/bmp",
        "TIFF"   => "image/tiff",
        "TXT"    => "text/plain",
        _        => "application/octet-stream",
    };
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    Ok(format!("{}:{}", mime, b64))
}

// ── Robust read helper ────────────────────────────────────────────────────────

/// Read up to `max_bytes` from `src` at the current position using 64 KB chunks.
/// Unlike `read_exact`, this never fails when fewer bytes are available — it just
/// returns whatever the device gave us.  Returns an error only when zero bytes
/// could be read at all.
fn read_bytes_robust(src: &mut std::fs::File, max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut data: Vec<u8> = Vec::with_capacity(max_bytes.min(64 * 1024 * 1024));
    let mut buf = vec![0u8; 65536]; // 64 KB chunks
    let mut remaining = max_bytes;
    while remaining > 0 {
        let to_read = buf.len().min(remaining);
        match src.read(&mut buf[..to_read]) {
            Ok(0) => break,
            Ok(n) => {
                data.extend_from_slice(&buf[..n]);
                remaining -= n;
            }
            Err(e) => {
                if data.is_empty() {
                    return Err(format!("Read failed: {}", e));
                }
                break; // partial read — return what we have
            }
        }
    }
    if data.is_empty() {
        return Err("No data at this offset (device may have changed since scan)".to_string());
    }
    Ok(data)
}



fn validate_bytes(data: &[u8], file_type: &str) -> ValidationStatus {
    if data.is_empty() {
        return ValidationStatus { is_valid: false, confidence: 0.0, details: "Empty data".into() };
    }
    if data.iter().all(|&b| b == 0) {
        return ValidationStatus { is_valid: false, confidence: 0.0, details: "Overwritten (all zeros)".into() };
    }

    match file_type {
        "JPEG" => {
            let soi = data.starts_with(&[0xFF, 0xD8, 0xFF]);
            let eoi = data.len() >= 2 && data[data.len() - 2..] == [0xFF, 0xD9];
            match (soi, eoi) {
                (true, true)  => ValidationStatus { is_valid: true,  confidence: 0.95, details: "Valid JPEG (SOI + EOI)".into() },
                (true, false) => ValidationStatus { is_valid: true,  confidence: 0.60, details: "Partial JPEG – truncated (no EOI)".into() },
                _             => ValidationStatus { is_valid: false, confidence: 0.10, details: "Invalid JPEG header".into() },
            }
        }
        "PNG" => {
            let sig = data.len() >= 8 && &data[..8] == b"\x89PNG\r\n\x1A\n";
            let iend = data.windows(4).any(|w| w == b"IEND");
            match (sig, iend) {
                (true, true)  => ValidationStatus { is_valid: true,  confidence: 0.97, details: "Valid PNG (signature + IEND)".into() },
                (true, false) => ValidationStatus { is_valid: true,  confidence: 0.65, details: "Partial PNG – truncated".into() },
                _             => ValidationStatus { is_valid: false, confidence: 0.10, details: "Invalid PNG".into() },
            }
        }
        "PDF" => {
            let hdr = data.starts_with(b"%PDF-");
            let eof = data.windows(5).any(|w| w == b"%%EOF");
            match (hdr, eof) {
                (true, true)  => ValidationStatus { is_valid: true,  confidence: 0.92, details: "Valid PDF".into() },
                (true, false) => ValidationStatus { is_valid: true,  confidence: 0.55, details: "Partial PDF – truncated".into() },
                _             => ValidationStatus { is_valid: false, confidence: 0.10, details: "Invalid PDF".into() },
            }
        }
        "DOCX" | "XLSX" | "PPTX" | "ZIP" => {
            let pk  = data.len() >= 4 && &data[..4] == b"PK\x03\x04";
            let eocd = data.windows(4).any(|w| w == b"PK\x05\x06");
            match (pk, eocd) {
                (true, true)  => ValidationStatus { is_valid: true,  confidence: 0.93, details: "Valid ZIP/Office archive".into() },
                (true, false) => ValidationStatus { is_valid: true,  confidence: 0.60, details: "Partial ZIP – truncated".into() },
                _             => ValidationStatus { is_valid: false, confidence: 0.10, details: "Invalid ZIP header".into() },
            }
        }
        "TXT" => {
            let n = data.len().min(1024);
            let p = data[..n].iter()
                .filter(|&&b| matches!(b, b'\t' | b'\n' | b'\r' | 0x20..=0x7E))
                .count();
            let pct = (p as f32 / n as f32 * 100.0) as u32;
            if pct >= 85 {
                ValidationStatus { is_valid: true, confidence: pct as f32 / 100.0, details: format!("{}% printable ASCII", pct) }
            } else {
                ValidationStatus { is_valid: false, confidence: 0.2, details: format!("Only {}% printable ASCII", pct) }
            }
        }
        _ => ValidationStatus { is_valid: true, confidence: 0.50, details: "Signature match only".into() },
    }
}

/// Opens a device or file for raw sequential reading (shared, no write access).
#[cfg(target_os = "windows")]
fn open_device_ro(path: &str) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)
        .open(path)
}

#[cfg(not(target_os = "windows"))]
fn open_device_ro(path: &str) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// Parse NTFS MFT for deleted file records
#[tauri::command]
async fn scan_mft(
    device_path: String,
    window: Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Command: scan_mft on {}", device_path);

    let mft_store = Arc::clone(&state.mft_results);
    let win = window.clone();

    thread::spawn(move || {
        let parser = MftParser::new(device_path);
        match parser.parse_deleted_entries() {
            Ok(entries) => {
                info!("MFT scan found {} deleted entries", entries.len());
                *mft_store.lock().unwrap() = entries.clone();
                let _ = win.emit("mft-complete", entries);
            }
            Err(e) => {
                error!("MFT scan error: {}", e);
                let _ = win.emit("mft-error", e.to_string());
            }
        }
    });

    Ok(())
}

/// Get cached MFT results
#[tauri::command]
async fn get_mft_results(state: State<'_, AppState>) -> Result<Vec<DeletedMftEntry>, String> {
    Ok(state.mft_results.lock().unwrap().clone())
}

/// Start a shred operation
/// Emits `shred-progress` events to the frontend
#[tauri::command]
async fn start_shred(
    target_path: String,
    algorithm: String,
    verify: bool,
    window: Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Command: start_shred on {} with {}", target_path, algorithm);

    state.shred_cancel.store(false, Ordering::SeqCst);

    let algo = match algorithm.as_str() {
        "DoD5220" => ShredAlgorithm::DoD5220,
        "Gutmann35" => ShredAlgorithm::Gutmann35,
        "RandomSingle" => ShredAlgorithm::RandomSingle,
        "NvmeSanitize" => ShredAlgorithm::NvmeSanitize,
        "NvmeFormat" => ShredAlgorithm::NvmeFormat,
        _ => return Err(format!("Unknown algorithm: {}", algorithm)),
    };

    let options = ShredOptions {
        algorithm: algo,
        verify_passes: verify,
        target_path: target_path.clone(),
    };

    let (tx, rx) = unbounded::<ShredProgress>();
    let cancel = Arc::clone(&state.shred_cancel);
    let win_clone = window.clone();

    // Shred thread
    thread::spawn(move || {
        let shredder = Shredder::new(options, tx, cancel);
        match shredder.execute() {
            Ok(_) => {
                info!("Shred complete on {}", target_path);
                let _ = win_clone.emit("shred-complete", &target_path);
            }
            Err(e) => {
                error!("Shred error: {}", e);
                let _ = win_clone.emit("shred-error", e.to_string());
            }
        }
    });

    // Progress relay
    let win_progress = window.clone();
    thread::spawn(move || {
        for progress in rx {
            let _ = win_progress.emit("shred-progress", &progress);
        }
    });

    Ok(())
}

/// Cancel an ongoing shred operation
#[tauri::command]
async fn cancel_shred(state: State<'_, AppState>) -> Result<(), String> {
    info!("Command: cancel_shred");
    state.shred_cancel.store(true, Ordering::SeqCst);
    Ok(())
}

/// Measure NVMe sequential read speed
#[tauri::command]
async fn measure_nvme_speed(device_path: String) -> Result<f64, String> {
    info!("Command: measure_nvme_speed on {}", device_path);
    modules::nvme::NvmeBandwidthEstimator::measure_sequential_read_mb(&device_path)
        .map_err(|e| e.to_string())
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .with_target(false)
        .compact()
        .init();

    info!("Aeon Data Systems starting...");

    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            list_disks,
            start_scan,
            cancel_scan,
            get_scan_results,
            recover_file,
            recover_batch,
            export_recovered_zip,
            preview_file,
            scan_mft,
            get_mft_results,
            start_shred,
            cancel_shred,
            measure_nvme_speed,
        ])
        .run(tauri::generate_context!())
        .expect("Error while running Aeon Data Systems");
}
