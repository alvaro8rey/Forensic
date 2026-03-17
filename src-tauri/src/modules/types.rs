use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveredFile {
    pub id: u64,
    pub file_type: FileType,
    pub offset_start: u64,
    pub offset_end: u64,
    pub size_bytes: u64,
    pub recovery_probability: f32,
    pub signature_matched: String,
    pub is_fragmented: bool,
    pub fragment_count: u32,
    pub sector_overwritten: bool,
    pub preview_available: bool,
    pub thumbnail_base64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileType {
    JPEG,
    PNG,
    PDF,
    DOCX,
    ZIP,
    EXE,
    MP4,
    MP3,
    Unknown(String),
}

impl std::fmt::Display for FileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileType::JPEG => write!(f, "JPEG"),
            FileType::PNG => write!(f, "PNG"),
            FileType::PDF => write!(f, "PDF"),
            FileType::DOCX => write!(f, "DOCX"),
            FileType::ZIP => write!(f, "ZIP"),
            FileType::EXE => write!(f, "EXE"),
            FileType::MP4 => write!(f, "MP4"),
            FileType::MP3 => write!(f, "MP3"),
            FileType::Unknown(ext) => write!(f, "{}", ext),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub bytes_scanned: u64,
    pub total_bytes: u64,
    pub current_offset_hex: String,
    pub files_found: u32,
    pub scan_speed_mb: f64,
    pub elapsed_seconds: u64,
    pub status: ScanStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScanStatus {
    Idle,
    Scanning,
    Paused,
    Completed,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub device_path: String,
    pub display_name: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub file_system: String,
    pub is_ssd: bool,
    pub smart_health: SmartHealth,
    pub model: String,
    pub serial: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartHealth {
    pub overall_health: HealthStatus,
    pub temperature_celsius: Option<u32>,
    pub reallocated_sectors: u32,
    pub pending_sectors: u32,
    pub uncorrectable_sectors: u32,
    pub power_on_hours: u64,
    pub health_score: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Warning,
    Critical,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShredOptions {
    pub algorithm: ShredAlgorithm,
    pub verify_passes: bool,
    pub target_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShredAlgorithm {
    /// DoD 5220.22-M: 3 passes (0x00, 0xFF, random)
    DoD5220,
    /// Gutmann: 35 passes with specific patterns
    Gutmann35,
    /// Single pass random (fast)
    RandomSingle,
    /// NVMe Secure Erase via ATA Sanitize command
    NvmeSanitize,
    /// NVMe Format NVM command
    NvmeFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShredProgress {
    pub current_pass: u32,
    pub total_passes: u32,
    pub bytes_written: u64,
    pub total_bytes: u64,
    pub algorithm: String,
    pub verification_passed: Option<bool>,
    pub status: ShredStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShredStatus {
    Idle,
    Shredding,
    Verifying,
    Completed,
    Error(String),
}

/// Magic byte signatures for file carving
pub struct FileSignature {
    pub header: Vec<u8>,
    pub footer: Option<Vec<u8>>,
    pub file_type: FileType,
    pub max_size: u64,
}

impl FileSignature {
    pub fn all_signatures() -> Vec<FileSignature> {
        vec![
            FileSignature {
                header: vec![0xFF, 0xD8, 0xFF],
                footer: Some(vec![0xFF, 0xD9]),
                file_type: FileType::JPEG,
                max_size: 50 * 1024 * 1024, // 50MB
            },
            FileSignature {
                header: vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
                footer: Some(vec![0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82]),
                file_type: FileType::PNG,
                max_size: 100 * 1024 * 1024, // 100MB
            },
            FileSignature {
                header: vec![0x25, 0x50, 0x44, 0x46, 0x2D], // %PDF-
                footer: Some(vec![0x25, 0x25, 0x45, 0x4F, 0x46]), // %%EOF
                file_type: FileType::PDF,
                max_size: 200 * 1024 * 1024, // 200MB
            },
            FileSignature {
                header: vec![0x50, 0x4B, 0x03, 0x04], // ZIP/DOCX/XLSX
                footer: Some(vec![0x50, 0x4B, 0x05, 0x06]),
                file_type: FileType::DOCX,
                max_size: 100 * 1024 * 1024,
            },
            FileSignature {
                header: vec![0x4D, 0x5A], // MZ - Windows PE
                footer: None,
                file_type: FileType::EXE,
                max_size: 500 * 1024 * 1024,
            },
            FileSignature {
                header: vec![0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70], // MP4
                footer: None,
                file_type: FileType::MP4,
                max_size: 4 * 1024 * 1024 * 1024, // 4GB
            },
            FileSignature {
                header: vec![0x49, 0x44, 0x33], // ID3 - MP3
                footer: None,
                file_type: FileType::MP3,
                max_size: 50 * 1024 * 1024,
            },
        ]
    }
}
