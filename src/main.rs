mod protocol;
mod synthesis;

use std::io::{self, BufRead, Write};
use std::path::Path;

use protocol::{TtsRequest, TtsResponse};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    // Initialize tracing (respects LINGOPILOT_TTS_LOG or RUST_LOG env vars)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("LINGOPILOT_TTS_LOG")
                .unwrap_or_else(|_| {
                    tracing_subscriber::EnvFilter::try_from_env("RUST_LOG")
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"))
                }),
        )
        .with_writer(io::stderr)
        .init();

    tracing::info!("lingopilot-tts-piper v{} starting", VERSION);

    // Send ready signal
    send_response(&TtsResponse::Ready {
        version: VERSION.to_string(),
    });

    // Main loop: read JSON requests from stdin, one per line
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("Failed to read stdin: {}", e);
                break;
            }
        };

        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: TtsRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                send_response(&TtsResponse::Error {
                    message: format!("Invalid JSON request: {}", e),
                });
                continue;
            }
        };

        handle_request(request);
    }

    tracing::info!("stdin closed, shutting down");
}

fn handle_request(req: TtsRequest) {
    tracing::debug!(
        "TTS request: voice={}, language={}, speed={}, text_len={}",
        req.voice,
        req.language,
        req.speed,
        req.text.len()
    );

    // Ensure eSpeak-NG is initialized
    let espeak_data = Path::new(&req.espeak_data_dir);
    if let Err(e) = synthesis::ensure_espeak_initialized(espeak_data) {
        send_response(&TtsResponse::Error {
            message: format!("eSpeak init failed: {}", e),
        });
        return;
    }

    // Find the voice config file
    let model_dir = Path::new(&req.model_dir);
    let config_path = match synthesis::find_voice_config(model_dir, &req.voice) {
        Ok(p) => p,
        Err(e) => {
            send_response(&TtsResponse::Error { message: e });
            return;
        }
    };

    // Synthesize
    match synthesis::synthesize(&req.text, &config_path, req.speed) {
        Ok(result) => {
            // Convert i16 samples to bytes (PCM16 LE)
            let byte_len = (result.pcm16.len() * 2) as u32;

            // Send JSON header
            send_response(&TtsResponse::Audio {
                byte_length: byte_len,
                sample_rate: result.sample_rate,
                channels: 1,
            });

            // Send raw PCM bytes immediately after
            let stdout = io::stdout();
            let mut out = stdout.lock();
            for sample in &result.pcm16 {
                let bytes = sample.to_le_bytes();
                if out.write_all(&bytes).is_err() {
                    tracing::error!("Failed to write audio to stdout");
                    return;
                }
            }
            if out.flush().is_err() {
                tracing::error!("Failed to flush stdout");
            }
        }
        Err(e) => {
            send_response(&TtsResponse::Error {
                message: format!("Synthesis failed: {}", e),
            });
        }
    }
}

/// Send a JSON response to stdout followed by a newline.
fn send_response(response: &TtsResponse) {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let json = serde_json::to_string(response).expect("Failed to serialize response");
    let _ = writeln!(out, "{}", json);
    let _ = out.flush();
}
