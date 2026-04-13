use serde::{Deserialize, Serialize};

/// Request sent by the host process via stdin (one JSON object per line).
#[derive(Debug, Deserialize)]
pub struct TtsRequest {
    /// Text to synthesize.
    pub text: String,

    /// eSpeak language code (e.g. "en", "de", "pt-br").
    pub language: String,

    /// Piper voice ID (e.g. "en_US-hfc_female-medium").
    pub voice: String,

    /// Playback speed multiplier (1.0 = normal).
    #[serde(default = "default_speed")]
    pub speed: f32,

    /// Piper model directory path (absolute).
    pub model_dir: String,

    /// eSpeak-NG data directory path (absolute).
    pub espeak_data_dir: String,
}

fn default_speed() -> f32 {
    1.0
}

/// Response sent back to the host process via stdout.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum TtsResponse {
    /// Successful synthesis — audio follows as binary after this JSON line.
    #[serde(rename = "audio")]
    Audio {
        /// Number of PCM16 LE bytes that follow on stdout after the newline.
        byte_length: u32,
        /// Sample rate of the audio (e.g. 22050).
        sample_rate: u32,
        /// Number of audio channels (always 1 — mono).
        channels: u16,
    },

    /// An error occurred during synthesis.
    #[serde(rename = "error")]
    Error {
        /// Human-readable error message.
        message: String,
    },

    /// Sidecar is ready to accept requests.
    #[serde(rename = "ready")]
    Ready {
        /// Sidecar version string.
        version: String,
    },
}
