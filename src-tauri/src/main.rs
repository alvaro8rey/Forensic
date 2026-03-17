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
    mft::{DeletedMftEntry, MftParser},
    shredder::Shredder,
    smart::SmartReader,
    types::{DiskInfo, RecoveredFile, ScanProgress, ShredAlgorithm, ShredOptions, ShredProgress},
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

/// Start a file-carving scan on a device
/// Emits `scan-progress` events to the frontend in real-time
#[tauri::command]
async fn start_scan(
    device_path: String,
    window: Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Command: start_scan on {}", device_path);

    // Reset cancel flag and store device path for later recovery
    state.scan_cancel.store(false, Ordering::SeqCst);
    *state.scan_device.lock().unwrap() = device_path.clone();

    let (tx, rx) = unbounded::<ScanProgress>();
    let cancel = Arc::clone(&state.scan_cancel);
    let results_store = Arc::clone(&state.scan_results);
    let win_clone = window.clone();
    let path_clone = device_path.clone();

    // Spawn scan thread (blocking I/O)
    thread::spawn(move || {
        let carver = FileCarver::new(path_clone, tx, cancel);
        match carver.scan() {
            Ok(files) => {
                info!("Scan returned {} files", files.len());
                *results_store.lock().unwrap() = files.clone();
                let _ = win_clone.emit("scan-complete", files);
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
/// The `key` matches a translation key in the frontend locales
/// (e.g. "recovery.recoveredMsg") so the UI can display a localised message.
#[derive(serde::Serialize)]
struct RecoverResult {
    key: String,
    file_type: String,
    kb: usize,
    path: String,
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

    // Open the source device with shared read access
    let mut src = open_device_ro(&device_path)
        .map_err(|e| format!("Cannot open device '{}': {}", device_path, e))?;

    src.seek(SeekFrom::Start(offset_start))
        .map_err(|e| format!("Seek failed: {}", e))?;

    let mut data = vec![0u8; read_size];
    src.read_exact(&mut data)
        .map_err(|e| format!("Read failed at offset 0x{:X}: {}", offset_start, e))?;

    // Write recovered bytes to the user-chosen destination
    let mut dst = std::fs::File::create(&destination_path)
        .map_err(|e| format!("Cannot create output file '{}': {}", destination_path, e))?;
    dst.write_all(&data)
        .map_err(|e| format!("Write failed: {}", e))?;

    let kb = read_size / 1024;
    info!("Recovered {} ({} KB) from 0x{:X} → {}", type_str, kb, offset_start, destination_path);
    Ok(RecoverResult {
        key: "recovery.recoveredMsg".to_string(),
        file_type: type_str,
        kb,
        path: destination_path,
    })
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
    let mut src = open_device_ro(&device_path)
        .map_err(|e| format!("Cannot open device: {}", e))?;
    src.seek(SeekFrom::Start(offset_start))
        .map_err(|e| format!("Seek failed: {}", e))?;

    let mut data = vec![0u8; read_size];
    src.read_exact(&mut data)
        .map_err(|e| format!("Read failed: {}", e))?;

    // Return "mime:base64data" so the frontend knows the content type
    let mime = match type_str.as_str() {
        "JPEG" => "image/jpeg",
        "PNG"  => "image/png",
        "GIF"  => "image/gif",
        _      => "application/octet-stream",
    };
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    Ok(format!("{}:{}", mime, b64))
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
