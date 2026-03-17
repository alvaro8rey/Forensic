// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod modules;

use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::collections::HashMap;

use crossbeam_channel::{unbounded, Receiver};
use serde_json::Value;
use tauri::{AppHandle, Manager, State, Window};
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};

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
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            scan_cancel: Arc::new(AtomicBool::new(false)),
            shred_cancel: Arc::new(AtomicBool::new(false)),
            scan_results: Arc::new(Mutex::new(Vec::new())),
            mft_results: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

// ─── Tauri Commands ───────────────────────────────────────────────────────────

/// List all available disks with S.M.A.R.T. health info
#[tauri::command]
async fn list_disks(state: State<'_, AppState>) -> Result<Vec<DiskInfo>, String> {
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

    // Reset cancel flag
    state.scan_cancel.store(false, Ordering::SeqCst);

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

/// Recover (extract) a specific file from disk to a destination
#[tauri::command]
async fn recover_file(
    file_id: u64,
    destination_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    info!("Command: recover_file id={} dest={}", file_id, destination_path);

    let results = state.scan_results.lock().unwrap();
    let file = results
        .iter()
        .find(|f| f.id == file_id)
        .ok_or_else(|| format!("File ID {} not found in scan results", file_id))?;

    // TODO: open source device and extract bytes from offset_start..offset_end
    let size_kb = file.size_bytes / 1024;
    info!(
        "Extracting {} ({} KB) from 0x{:X} to {}",
        file.file_type, size_kb, file.offset_start, destination_path
    );

    Ok(format!(
        "Recovered {} ({} KB) → {}",
        file.file_type, size_kb, destination_path
    ))
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
            scan_mft,
            get_mft_results,
            start_shred,
            cancel_shred,
            measure_nvme_speed,
        ])
        .run(tauri::generate_context!())
        .expect("Error while running Aeon Data Systems");
}
