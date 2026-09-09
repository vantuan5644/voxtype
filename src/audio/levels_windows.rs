//! Windows placeholder for the Unix-domain-socket OSD audio feed.
//!
//! The Windows package does not ship the Wayland OSD.  These types retain the
//! shared daemon interfaces while keeping audio capture and streaming active.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::mpsc;

pub const FRAME_HZ: usize = 100;
pub const FRAME_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AudioFrame {
    pub seq: u32,
    pub min: f32,
    pub max: f32,
    pub peak_dbfs: f32,
}

impl AudioFrame {
    pub fn to_bytes(self) -> [u8; FRAME_BYTES] {
        let mut buf = [0u8; FRAME_BYTES];
        buf[0..4].copy_from_slice(&self.seq.to_ne_bytes());
        buf[4..8].copy_from_slice(&self.min.to_ne_bytes());
        buf[8..12].copy_from_slice(&self.max.to_ne_bytes());
        buf[12..16].copy_from_slice(&self.peak_dbfs.to_ne_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8; FRAME_BYTES]) -> Self {
        Self {
            seq: u32::from_ne_bytes(bytes[0..4].try_into().unwrap()),
            min: f32::from_ne_bytes(bytes[4..8].try_into().unwrap()),
            max: f32::from_ne_bytes(bytes[8..12].try_into().unwrap()),
            peak_dbfs: f32::from_ne_bytes(bytes[12..16].try_into().unwrap()),
        }
    }
}

pub fn default_socket_path() -> PathBuf {
    crate::config::Config::runtime_dir().join("audio.sock")
}

#[derive(Clone, Default)]
pub struct FrameSink;

impl FrameSink {
    pub fn publish(&self, _frame: AudioFrame) {}
}

#[derive(Clone)]
pub struct LevelHub {
    socket_path: PathBuf,
}

impl LevelHub {
    pub async fn start(socket_path: PathBuf) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "the OSD audio socket is unavailable on Windows ({})",
                socket_path.display()
            ),
        ))
    }

    pub fn frame_sink(&self) -> FrameSink {
        FrameSink
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn cleanup(&self) {}
}

pub fn spawn_emitter(
    chunk_rx: mpsc::Receiver<Vec<f32>>,
    sink: FrameSink,
) -> tokio::task::JoinHandle<()> {
    spawn_emitter_with_streaming_tap(chunk_rx, sink, None)
}

pub fn spawn_emitter_with_streaming_tap(
    mut chunk_rx: mpsc::Receiver<Vec<f32>>,
    _sink: FrameSink,
    streaming_tx: Option<mpsc::Sender<Vec<f32>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(chunk) = chunk_rx.recv().await {
            if let Some(ref tx) = streaming_tx {
                let _ = tx.try_send(chunk);
            }
        }
    })
}

pub fn spawn_silence_pump(_sink: FrameSink) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    })
}
