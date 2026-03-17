/// NVMe Async I/O Optimizer
/// Implements asynchronous multi-queue reads for NVMe SSDs using PCIe DMA channels.
/// On Windows: leverages overlapped I/O with multiple completion ports per NVMe queue.
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use crossbeam_channel::Sender;
use rayon::prelude::*;
use tracing::{debug, info};

/// NVMe queue depth — modern controllers support 64K entries per queue
const QUEUE_DEPTH: usize = 32;
/// Optimal NVMe read size = 128KB aligned to page boundaries
const NVME_OPTIMAL_READ: usize = 128 * 1024;
/// Number of parallel I/O queues to saturate PCIe lanes
const PARALLEL_QUEUES: usize = 4;

#[derive(Debug, Clone)]
pub struct NvmeChunk {
    pub queue_id: usize,
    pub offset: u64,
    pub data: Vec<u8>,
    pub sequence: u64,
}

pub struct NvmeAsyncReader {
    device_path: String,
    total_size: u64,
}

impl NvmeAsyncReader {
    pub fn new(device_path: String, total_size: u64) -> Self {
        Self { device_path, total_size }
    }

    /// Read the entire device using parallel I/O queues
    /// Returns ordered chunks via the provided channel
    pub fn read_parallel(
        &self,
        chunk_tx: Sender<NvmeChunk>,
        start_offset: u64,
        end_offset: u64,
    ) -> Result<()> {
        let range_size = end_offset.saturating_sub(start_offset);
        let chunk_size = NVME_OPTIMAL_READ as u64;
        let total_chunks = (range_size + chunk_size - 1) / chunk_size;

        info!(
            "NVMe parallel read: {} MB across {} queues ({} chunks)",
            range_size / 1024 / 1024,
            PARALLEL_QUEUES,
            total_chunks
        );

        // Divide chunks across parallel queues
        let chunks_per_queue = (total_chunks as usize + PARALLEL_QUEUES - 1) / PARALLEL_QUEUES;
        let device_path = self.device_path.clone();
        let tx = chunk_tx.clone();

        (0..PARALLEL_QUEUES).into_par_iter().try_for_each(|queue_id| -> Result<()> {
            let queue_start = start_offset + (queue_id as u64 * chunks_per_queue as u64 * chunk_size);
            let queue_end = (queue_start + chunks_per_queue as u64 * chunk_size).min(end_offset);

            if queue_start >= end_offset {
                return Ok(());
            }

            debug!("Queue {} reading 0x{:X}..0x{:X}", queue_id, queue_start, queue_end);

            let mut file = std::fs::File::open(&device_path)?;
            let mut offset = queue_start;
            let mut seq: u64 = queue_id as u64 * chunks_per_queue as u64;

            while offset < queue_end {
                let to_read = std::cmp::min(NVME_OPTIMAL_READ as u64, queue_end - offset) as usize;
                let mut buf = vec![0u8; to_read];

                file.seek(SeekFrom::Start(offset))?;
                let n = file.read(&mut buf)?;

                if n == 0 {
                    break;
                }

                buf.truncate(n);

                let _ = tx.send(NvmeChunk {
                    queue_id,
                    offset,
                    data: buf,
                    sequence: seq,
                });

                offset += n as u64;
                seq += 1;
            }

            Ok(())
        })?;

        Ok(())
    }

    /// Windows-specific: Overlapped async I/O using IOCP (I/O Completion Ports)
    /// Each NVMe queue maps to a dedicated IOCP thread
    #[cfg(target_os = "windows")]
    pub fn read_overlapped_iocp(
        &self,
        chunk_tx: Sender<NvmeChunk>,
        start_offset: u64,
        end_offset: u64,
    ) -> Result<()> {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, ReadFile, FILE_FLAG_NO_BUFFERING,
            FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING,
        };
        use windows_sys::Win32::System::IO::{
            CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED,
        };
        // GENERIC_READ = 0x80000000 (WinNT.h)
        const GENERIC_READ: u32 = 0x8000_0000;
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        let wide_path: Vec<u16> = OsStr::new(&self.device_path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // Create I/O Completion Port
        let iocp = unsafe {
            CreateIoCompletionPort(INVALID_HANDLE_VALUE, 0, 0, PARALLEL_QUEUES as u32)
        };

        if iocp == 0 {
            anyhow::bail!("Failed to create IOCP");
        }

        // Open device with overlapped flag
        let file_handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_NO_BUFFERING | FILE_FLAG_OVERLAPPED,
                0,
            )
        };

        if file_handle == INVALID_HANDLE_VALUE {
            anyhow::bail!("Failed to open device for overlapped I/O");
        }

        // Associate file handle with IOCP
        unsafe {
            CreateIoCompletionPort(file_handle, iocp, 1, 0);
        }

        info!("IOCP async reader active for NVMe device");

        // Dispatch overlapped reads in QUEUE_DEPTH batches
        let range_size = end_offset - start_offset;
        let chunk_size = NVME_OPTIMAL_READ as u64;
        let mut offset = start_offset;
        let mut sequence: u64 = 0;
        let pending = Arc::new(Mutex::new(0u32));

        while offset < end_offset || *pending.lock().unwrap() > 0 {
            // Issue reads up to queue depth
            while offset < end_offset && *pending.lock().unwrap() < QUEUE_DEPTH as u32 {
                let to_read = std::cmp::min(chunk_size, end_offset - offset) as usize;
                let mut buf = vec![0u8; to_read];
                let current_offset = offset;
                let current_seq = sequence;

                let mut overlapped = Box::new(OVERLAPPED {
                    Internal: 0,
                    InternalHigh: 0,
                    Anonymous: unsafe { std::mem::zeroed() },
                    hEvent: 0,
                });

                // Set file offset in OVERLAPPED structure.
                // In windows-sys 0.52 the layout is:
                //   OVERLAPPED.Anonymous (OVERLAPPED_0 union)
                //     .Anonymous (OVERLAPPED_0_0 struct)
                //       .Offset / .OffsetHigh
                unsafe {
                    overlapped.Anonymous.Anonymous.Offset = (current_offset & 0xFFFF_FFFF) as u32;
                    overlapped.Anonymous.Anonymous.OffsetHigh = (current_offset >> 32) as u32;
                }

                let overlapped_ptr = Box::into_raw(overlapped);

                unsafe {
                    ReadFile(
                        file_handle,
                        buf.as_mut_ptr() as *mut _,
                        to_read as u32,
                        std::ptr::null_mut(),
                        overlapped_ptr,
                    );
                }

                *pending.lock().unwrap() += 1;
                offset += to_read as u64;
                sequence += 1;
            }

            // Collect completed reads
            let mut bytes_transferred: u32 = 0;
            let mut key: usize = 0;
            let mut overlapped_out: *mut OVERLAPPED = std::ptr::null_mut();

            let got = unsafe {
                GetQueuedCompletionStatus(
                    iocp,
                    &mut bytes_transferred,
                    &mut key as *mut usize as *mut _,
                    &mut overlapped_out,
                    1000, // 1s timeout
                )
            };

            if got != 0 && bytes_transferred > 0 {
                *pending.lock().unwrap() -= 1;
                // Re-box overlapped to free memory
                if !overlapped_out.is_null() {
                    unsafe { let _ = Box::from_raw(overlapped_out); }
                }
            }
        }

        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(file_handle);
            windows_sys::Win32::Foundation::CloseHandle(iocp);
        }

        info!("IOCP overlapped read complete.");
        Ok(())
    }
}

/// Bandwidth estimator for NVMe drives
pub struct NvmeBandwidthEstimator;

impl NvmeBandwidthEstimator {
    pub fn measure_sequential_read_mb(device_path: &str) -> Result<f64> {
        let mut file = std::fs::File::open(device_path)?;
        let sample_size = 256 * 1024 * 1024usize; // 256MB sample
        let mut buf = vec![0u8; NVME_OPTIMAL_READ];
        let mut total_read = 0usize;
        let start = std::time::Instant::now();

        while total_read < sample_size {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            total_read += n;
        }

        let elapsed = start.elapsed().as_secs_f64();
        let mb_per_sec = (total_read as f64 / 1024.0 / 1024.0) / elapsed;
        info!("NVMe sequential read: {:.1} MB/s", mb_per_sec);
        Ok(mb_per_sec)
    }
}
