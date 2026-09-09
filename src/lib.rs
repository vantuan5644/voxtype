//! Voxtype: native push-to-talk voice-to-text.
//!
//! This library provides the core functionality for:
//! - Detecting global hotkey presses through platform-native APIs
//! - Capturing audio through cpal
//! - Transcribing speech using whisper.cpp (fast, local, offline)
//! - Processing text (punctuation, replacements, optional LLM post-processing)
//! - Outputting text through native keyboard and clipboard APIs
//!
//! # Architecture
//!
//! ```text
//!                            ┌─────────────────────────────────────┐
//!                            │              Daemon                 │
//!                            └─────────────────────────────────────┘
//!                                            │
//!                   ┌────────────────────────┼────────────────────────┐
//!                   │                        │                        │
//!                   ▼                        ▼                        ▼
//!          ┌──────────────┐         ┌──────────────┐         ┌──────────────┐
//!          │    Hotkey    │         │    Audio     │         │    State     │
//!          │  (platform)  │         │    (cpal)    │         │   Manager    │
//!          └──────────────┘         └──────────────┘         └──────────────┘
//!                   │                        │
//!                   │  key press             │ audio samples
//!                   │  key release           │
//!                   ▼                        ▼
//!          ┌─────────────────────────────────────────────────────────────────┐
//!          │                        Recording Flow                           │
//!          │  [Press] ──▶ Start Recording ──▶ [Release] ──▶ Stop & Process   │
//!          └─────────────────────────────────────────────────────────────────┘
//!                                            │
//!                                            ▼
//!                                   ┌──────────────┐
//!                                   │   Whisper    │
//!                                   │ (whisper-rs) │
//!                                   └──────────────┘
//!                                            │
//!                                            ▼ raw text
//!                                   ┌──────────────┐
//!                                   │     Text     │
//!                                   │  Processing  │
//!                                   └──────────────┘
//!                                            │
//!                                            ▼ processed text
//!                                   ┌──────────────┐
//!                                   │ Post-Process │ (optional: LLM cleanup)
//!                                   │   Command    │
//!                                   └──────────────┘
//!                                            │
//!                                            ▼ final text
//!                                   ┌──────────────┐
//!                                   │  Pre-Output  │ (optional: compositor hook)
//!                                   │    Hook      │
//!                                   └──────────────┘
//!                                            │
//!                                            ▼
//!                                   ┌──────────────┐
//!                                   │    Output    │
//!                                   │   native /   │
//!                                   │  clipboard   │
//!                                   └──────────────┘
//!                                            │
//!                                            ▼
//!                                   ┌──────────────┐
//!                                   │ Post-Output  │ (optional: compositor hook)
//!                                   │    Hook      │
//!                                   └──────────────┘
//! ```

pub mod audio;
pub mod cli;
pub mod config;
pub mod config_set;
pub mod cpu;
pub mod daemon;
pub mod daemon_status;
pub mod eager;
pub mod error;
#[cfg(target_os = "linux")]
pub mod hotkey;
#[cfg(target_os = "macos")]
pub mod hotkey_macos;
#[cfg(target_os = "windows")]
pub mod hotkey_windows;
pub mod meeting;
#[cfg(target_os = "macos")]
pub mod menubar;
pub mod model_catalog;
pub mod model_manager;
pub mod notification;
pub mod osd;
pub mod output;
pub mod platform;
pub mod setup;
pub mod state;
pub mod status_json;
pub mod text;
pub mod transcribe;
pub mod tui;
pub mod vad;

pub use cli::{
    Cli, Commands, CompositorType, ConfigAction, InfoAction, MeetingAction, OutputModeOverride,
    RecordAction, SetupAction,
};
pub use config::Config;
pub use daemon::Daemon;
pub use error::{Result, VoxtypeError};
