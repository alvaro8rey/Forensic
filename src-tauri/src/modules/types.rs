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
    /// Original filename from FAT32/exFAT directory entry, if available.
    #[serde(default)]
    pub original_name: Option<String>,
}

/// Result of validating a recovered file's byte content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationStatus {
    pub is_valid: bool,
    pub confidence: f32,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileType {
    JPEG,
    PNG,
    GIF,
    TIFF,
    BMP,
    PDF,
    DOCX,   // ZIP-based Word
    XLSX,   // ZIP-based Excel
    PPTX,   // ZIP-based PowerPoint
    DOC,    // OLE2 compound (DOC/XLS/PPT)
    ZIP,    // Generic ZIP archive
    RAR,
    SevenZ,
    EXE,
    MP4,
    AVI,
    MKV,
    MP3,
    WAV,
    FLAC,
    SQLite,
    TXT,
    Unknown(String),
}

impl std::fmt::Display for FileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileType::JPEG   => write!(f, "JPEG"),
            FileType::PNG    => write!(f, "PNG"),
            FileType::GIF    => write!(f, "GIF"),
            FileType::TIFF   => write!(f, "TIFF"),
            FileType::BMP    => write!(f, "BMP"),
            FileType::PDF    => write!(f, "PDF"),
            FileType::DOCX   => write!(f, "DOCX"),
            FileType::XLSX   => write!(f, "XLSX"),
            FileType::PPTX   => write!(f, "PPTX"),
            FileType::DOC    => write!(f, "DOC"),
            FileType::ZIP    => write!(f, "ZIP"),
            FileType::RAR    => write!(f, "RAR"),
            FileType::SevenZ => write!(f, "7Z"),
            FileType::EXE    => write!(f, "EXE"),
            FileType::MP4    => write!(f, "MP4"),
            FileType::AVI    => write!(f, "AVI"),
            FileType::MKV    => write!(f, "MKV"),
            FileType::MP3    => write!(f, "MP3"),
            FileType::WAV    => write!(f, "WAV"),
            FileType::FLAC   => write!(f, "FLAC"),
            FileType::SQLite => write!(f, "SQLite"),
            FileType::TXT    => write!(f, "TXT"),
            FileType::Unknown(ext) => write!(f, "{}", ext),
        }
    }
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
pub struct ScanProgress {
    pub bytes_scanned: u64,
    pub total_bytes: u64,
    pub current_offset_hex: String,
    pub files_found: u32,
    pub scan_speed_mb: f64,
    pub elapsed_seconds: u64,
    pub status: ScanStatus,
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
    /// Schneier 7-pass: 0x00, 0xFF, then 5 random passes
    Schneier7,
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

/// Magic byte signatures for file carving.
pub struct FileSignature {
    pub header: Vec<u8>,
    pub footer: Option<Vec<u8>>,
    pub file_type: FileType,
    pub max_size: u64,
    /// Optional secondary check: (offset_from_match_start, expected_bytes).
    /// After matching `header`, verifies that the bytes at the given offset
    /// equal `expected_bytes`.  Used to disambiguate RIFF variants (AVI vs WAV),
    /// eliminate BMP false-positives, etc.
    pub verify: Option<(usize, Vec<u8>)>,
}

impl FileSignature {
    pub fn all_signatures() -> Vec<FileSignature> {
        vec![
            // ── Images ───────────────────────────────────────────────────────
            FileSignature {
                header: vec![0xFF, 0xD8, 0xFF],
                footer: Some(vec![0xFF, 0xD9]),
                file_type: FileType::JPEG,
                max_size: 50 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                header: vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
                footer: Some(vec![0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82]),
                file_type: FileType::PNG,
                max_size: 100 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                // GIF87a or GIF89a
                header: vec![0x47, 0x49, 0x46, 0x38],
                footer: Some(vec![0x00, 0x3B]),
                file_type: FileType::GIF,
                max_size: 20 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                // TIFF little-endian (II)
                header: vec![0x49, 0x49, 0x2A, 0x00],
                footer: None,
                file_type: FileType::TIFF,
                max_size: 200 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                // TIFF big-endian (MM)
                header: vec![0x4D, 0x4D, 0x00, 0x2A],
                footer: None,
                file_type: FileType::TIFF,
                max_size: 200 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                // BMP: "BM" magic + bytes 6–9 (reserved) must be zero
                header: vec![0x42, 0x4D],
                footer: None,
                file_type: FileType::BMP,
                max_size: 50 * 1024 * 1024,
                verify: Some((6, vec![0x00, 0x00, 0x00, 0x00])),
            },

            // ── Documents ────────────────────────────────────────────────────
            FileSignature {
                header: vec![0x25, 0x50, 0x44, 0x46, 0x2D], // %PDF-
                footer: Some(vec![0x25, 0x25, 0x45, 0x4F, 0x46]), // %%EOF
                file_type: FileType::PDF,
                max_size: 200 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                // ZIP-based Office (DOCX/XLSX/PPTX) and plain ZIP
                header: vec![0x50, 0x4B, 0x03, 0x04],
                footer: Some(vec![0x50, 0x4B, 0x05, 0x06]),
                file_type: FileType::DOCX,
                max_size: 100 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                // OLE2 Compound Document (Word/Excel/PowerPoint 97–2003)
                header: vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
                footer: None,
                file_type: FileType::DOC,
                max_size: 100 * 1024 * 1024,
                verify: None,
            },

            // ── Archives ─────────────────────────────────────────────────────
            FileSignature {
                // RAR 1.5–4.x
                header: vec![0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00],
                footer: None,
                file_type: FileType::RAR,
                max_size: 2 * 1024 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                // RAR 5.0+
                header: vec![0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00],
                footer: None,
                file_type: FileType::RAR,
                max_size: 2 * 1024 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                // 7-Zip
                header: vec![0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C],
                footer: None,
                file_type: FileType::SevenZ,
                max_size: 2 * 1024 * 1024 * 1024,
                verify: None,
            },

            // ── Executables ──────────────────────────────────────────────────
            FileSignature {
                // MZ + standard DOS stub (filters random MZ false-positives)
                header: vec![0x4D, 0x5A, 0x90, 0x00],
                footer: None,
                file_type: FileType::EXE,
                max_size: 50 * 1024 * 1024,
                verify: None,
            },

            // ── Video ─────────────────────────────────────────────────────────
            // MP4/MOV/M4V: ISO Base Media ftyp box — four common box sizes
            FileSignature {
                header: vec![0x00, 0x00, 0x00, 0x14, 0x66, 0x74, 0x79, 0x70],
                footer: None,
                file_type: FileType::MP4,
                max_size: 4 * 1024 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                header: vec![0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70],
                footer: None,
                file_type: FileType::MP4,
                max_size: 4 * 1024 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                header: vec![0x00, 0x00, 0x00, 0x1C, 0x66, 0x74, 0x79, 0x70],
                footer: None,
                file_type: FileType::MP4,
                max_size: 4 * 1024 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                header: vec![0x00, 0x00, 0x00, 0x20, 0x66, 0x74, 0x79, 0x70],
                footer: None,
                file_type: FileType::MP4,
                max_size: 4 * 1024 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                // AVI: RIFF container — verify "AVI " at offset 8
                header: vec![0x52, 0x49, 0x46, 0x46], // RIFF
                footer: None,
                file_type: FileType::AVI,
                max_size: 4 * 1024 * 1024 * 1024,
                verify: Some((8, vec![0x41, 0x56, 0x49, 0x20])), // "AVI "
            },
            FileSignature {
                // MKV / WebM: EBML header
                header: vec![0x1A, 0x45, 0xDF, 0xA3],
                footer: None,
                file_type: FileType::MKV,
                max_size: 4 * 1024 * 1024 * 1024,
                verify: None,
            },

            // ── Audio ─────────────────────────────────────────────────────────
            FileSignature {
                // MP3 with ID3v2.3 tag
                header: vec![0x49, 0x44, 0x33, 0x03],
                footer: None,
                file_type: FileType::MP3,
                max_size: 50 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                // MP3 with ID3v2.4 tag
                header: vec![0x49, 0x44, 0x33, 0x04],
                footer: None,
                file_type: FileType::MP3,
                max_size: 50 * 1024 * 1024,
                verify: None,
            },
            FileSignature {
                // WAV: RIFF container — verify "WAVE" at offset 8
                header: vec![0x52, 0x49, 0x46, 0x46], // RIFF
                footer: None,
                file_type: FileType::WAV,
                max_size: 500 * 1024 * 1024,
                verify: Some((8, vec![0x57, 0x41, 0x56, 0x45])), // "WAVE"
            },
            FileSignature {
                // FLAC: "fLaC" stream marker
                header: vec![0x66, 0x4C, 0x61, 0x43],
                footer: None,
                file_type: FileType::FLAC,
                max_size: 500 * 1024 * 1024,
                verify: None,
            },

            // ── Database ──────────────────────────────────────────────────────
            FileSignature {
                // SQLite 3 — 16-byte header string
                header: vec![
                    0x53, 0x51, 0x4C, 0x69, 0x74, 0x65, 0x20, 0x66,
                    0x6F, 0x72, 0x6D, 0x61, 0x74, 0x20, 0x33, 0x00,
                ],
                footer: None,
                file_type: FileType::SQLite,
                max_size: 512 * 1024 * 1024,
                verify: None,
            },
        ]
    }
}
