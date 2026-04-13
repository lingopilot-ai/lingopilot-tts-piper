use std::ffi::CString;
use std::path::Path;
use std::sync::OnceLock;

use piper_rs::from_config_path;
use piper_rs::synth::{AudioOutputConfig, PiperSpeechSynthesizer};

static ESPEAK_INIT: OnceLock<Result<(), String>> = OnceLock::new();

/// Result of a successful TTS synthesis.
pub struct SynthResult {
    /// Raw PCM16 LE samples (mono).
    pub pcm16: Vec<i16>,
    /// Sample rate reported by the Piper model.
    pub sample_rate: u32,
}

/// Initialize eSpeak-NG with the given data directory.
/// Safe to call multiple times — initialization is idempotent.
pub fn ensure_espeak_initialized(data_dir: &Path) -> Result<(), String> {
    let result = ESPEAK_INIT.get_or_init(|| {
        let path_cstr = CString::new(data_dir.to_string_lossy().as_bytes())
            .map_err(|e| format!("Invalid eSpeak data path: {}", e))?;

        let result = unsafe {
            espeak_rs_sys::espeak_Initialize(
                espeak_rs_sys::espeak_AUDIO_OUTPUT_AUDIO_OUTPUT_RETRIEVAL,
                0,
                path_cstr.as_ptr(),
                0,
            )
        };

        if result == -1 {
            return Err("eSpeak initialization failed".to_string());
        }

        tracing::info!("eSpeak-NG initialized (sample rate: {})", result);
        Ok(())
    });

    result.as_ref().map_err(Clone::clone).copied()
}

/// Convert a speed multiplier (1.0 = normal) to a piper-rs rate percentage (0-100).
/// piper-rs rate range is 0.5..5.5, mapped to 0..100.
/// A speed of 1.0 maps to ~10% in piper-rs terms (the default).
fn speed_to_rate_percent(speed: f32) -> Option<u8> {
    if (speed - 1.0).abs() < 0.01 {
        return None; // Use default speed
    }
    // Map speed multiplier to the 0-100 range that piper-rs uses
    // Rate range in piper-rs: 0.5..5.5 -> 0..100
    let percent = ((speed - 0.5) / (5.5 - 0.5) * 100.0).clamp(0.0, 100.0) as u8;
    Some(percent)
}

/// Synthesize text to PCM16 audio using Piper TTS.
pub fn synthesize(text: &str, voice_config_path: &Path, speed: f32) -> Result<SynthResult, String> {
    let model = from_config_path(voice_config_path)
        .map_err(|e| format!("Failed to load Piper voice: {}", e))?;

    let synth = PiperSpeechSynthesizer::new(model)
        .map_err(|e| format!("Failed to create synthesizer: {}", e))?;

    let audio_cfg = AudioOutputConfig {
        rate: speed_to_rate_percent(speed),
        volume: None,
        pitch: None,
        appended_silence_ms: None,
    };

    let audio_info = synth
        .clone_model()
        .audio_output_info()
        .map_err(|e| format!("Failed to get audio info: {}", e))?;

    let sample_rate = audio_info.sample_rate as u32;

    // Collect all audio samples from the parallel synthesizer
    let mut all_samples_f32: Vec<f32> = Vec::new();

    let stream = synth
        .synthesize_parallel(text.to_string(), Some(audio_cfg))
        .map_err(|e| format!("Synthesis failed: {}", e))?;

    for result in stream {
        match result {
            Ok(samples) => {
                all_samples_f32.extend(samples.into_vec());
            }
            Err(e) => {
                return Err(format!("Synthesis chunk failed: {}", e));
            }
        }
    }

    if all_samples_f32.is_empty() {
        return Err("Synthesis produced no audio".to_string());
    }

    // Convert f32 samples to i16 (PCM16)
    let pcm16: Vec<i16> = all_samples_f32
        .iter()
        .map(|&s| {
            let clamped = s.clamp(-1.0, 1.0);
            (clamped * 32767.0) as i16
        })
        .collect();

    Ok(SynthResult { pcm16, sample_rate })
}

/// Locate the Piper voice config file (`.onnx.json`) inside a model directory.
pub fn find_voice_config(model_dir: &Path, voice_id: &str) -> Result<std::path::PathBuf, String> {
    // Try standard Piper naming: <voice_id>.onnx.json
    let config_path = model_dir.join(format!("{}.onnx.json", voice_id));
    if config_path.exists() {
        return Ok(config_path);
    }

    // Try searching for any .onnx.json in the directory
    let entries = std::fs::read_dir(model_dir)
        .map_err(|e| format!("Cannot read model dir '{}': {}", model_dir.display(), e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(".onnx.json") {
                return Ok(path);
            }
        }
    }

    Err(format!(
        "No .onnx.json config found for voice '{}' in '{}'",
        voice_id,
        model_dir.display()
    ))
}
