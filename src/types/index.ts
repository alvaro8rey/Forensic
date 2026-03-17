export interface RecoveredFile {
  id: number;
  file_type: string;
  offset_start: number;
  offset_end: number;
  size_bytes: number;
  recovery_probability: number;
  signature_matched: string;
  is_fragmented: boolean;
  fragment_count: number;
  sector_overwritten: boolean;
  preview_available: boolean;
  thumbnail_base64: string | null;
}

export interface ScanProgress {
  bytes_scanned: number;
  total_bytes: number;
  current_offset_hex: string;
  files_found: number;
  scan_speed_mb: number;
  elapsed_seconds: number;
  status: "Idle" | "Scanning" | "Paused" | "Completed" | { Error: string };
}

export interface DiskInfo {
  device_path: string;
  display_name: string;
  total_bytes: number;
  used_bytes: number;
  file_system: string;
  is_ssd: boolean;
  smart_health: SmartHealth;
  model: string;
  serial: string;
}

export interface SmartHealth {
  overall_health: "Healthy" | "Warning" | "Critical" | "Unknown";
  temperature_celsius: number | null;
  reallocated_sectors: number;
  pending_sectors: number;
  uncorrectable_sectors: number;
  power_on_hours: number;
  health_score: number;
}

export interface ShredProgress {
  current_pass: number;
  total_passes: number;
  bytes_written: number;
  total_bytes: number;
  algorithm: string;
  verification_passed: boolean | null;
  status: "Idle" | "Shredding" | "Verifying" | "Completed" | { Error: string };
}

export interface DeletedMftEntry {
  record_number: number;
  file_name: string | null;
  file_size: number;
  created_time: number;
  modified_time: number;
  data_run_offset: number;
  is_recoverable: boolean;
  file_type_hint: string;
}

/** Structured result returned by the recover_file Tauri command */
export interface RecoverResult {
  key: string;
  file_type: string;
  kb: number;
  path: string;
}

export type AppView = "hunter" | "oblivion" | "dashboard";
export type ScanState = "idle" | "scanning" | "complete" | "error";
export type ShredState = "idle" | "shredding" | "complete" | "error";
