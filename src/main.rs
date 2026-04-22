mod protocol;
mod synthesis;

use std::fmt;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use protocol::{TtsRequest, TtsResponse};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::Writer as FormatWriter;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const ESPEAK_DATA_ENV: &str = "PIPER_ESPEAKNG_DATA_DIRECTORY";
const ESPEAK_RUNTIME_DIR_NAME: &str = "espeak-runtime";

struct ObservabilityFormatter;

#[derive(Default)]
struct KeyValueVisitor {
    fields: Vec<String>,
}

impl KeyValueVisitor {
    fn push(&mut self, key: &str, value: String) {
        self.fields.push(format!("{key}={value}"));
    }
}

impl Visit for KeyValueVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.push(field.name(), format_string_value(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.push(field.name(), value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.push(field.name(), value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.push(field.name(), value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.push(field.name(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.push(field.name(), format!("{value:?}"));
    }
}

impl<S, N> FormatEvent<S, N> for ObservabilityFormatter
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: FormatWriter<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut visitor = KeyValueVisitor::default();
        event.record(&mut visitor);

        write!(writer, "level={}", event.metadata().level())?;
        for field in visitor.fields {
            write!(writer, " {field}")?;
        }
        writeln!(writer)
    }
}

fn format_string_value(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '/'))
    {
        value.to_string()
    } else {
        format!("{value:?}")
    }
}

fn main() -> ExitCode {
    // Initialize tracing (respects PIPER_TTS_LOG or RUST_LOG env vars)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("PIPER_TTS_LOG").unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::try_from_env("RUST_LOG")
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"))
            }),
        )
        .with_writer(io::stderr)
        .with_ansi(false)
        .event_format(ObservabilityFormatter)
        .init();

    tracing::info!(event = "startup", version = VERSION);

    if std::env::args_os().len() > 1 {
        eprintln!(
            "Startup error: this sidecar takes no arguments; it auto-discovers the eSpeak runtime \
             next to the binary."
        );
        return ExitCode::FAILURE;
    }

    let espeak_data_dir = match discover_espeak_data_dir() {
        Ok(dir) => dir,
        Err(message) => {
            eprintln!("Startup error: {}", message);
            return ExitCode::FAILURE;
        }
    };

    std::env::set_var(ESPEAK_DATA_ENV, &espeak_data_dir);
    tracing::info!(
        event = "espeak_runtime_selected",
        espeak_data_dir = espeak_data_dir.display().to_string()
    );

    // Send ready signal
    if !send_response(
        &TtsResponse::Ready {
            version: VERSION.to_string(),
        },
        "ready",
    ) {
        return ExitCode::FAILURE;
    }

    let mut synthesis_cache = synthesis::SynthesisCache::new();

    // Main loop: read JSON requests from stdin, one per line
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(event = "stdin_read_failed", error = e.to_string());
                break;
            }
        };

        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: TtsRequest = match parse_request(&line) {
            Ok(r) => r,
            Err(message) => {
                let category = if message.starts_with("Invalid JSON request:") {
                    "invalid_json"
                } else {
                    "invalid_request_payload"
                };
                tracing::warn!(
                    event = "request_rejected",
                    category,
                    line_len = line.chars().count(),
                    detail = message.as_str()
                );
                let _ = send_response(&TtsResponse::Error { message }, "error");
                continue;
            }
        };

        handle_request(&mut synthesis_cache, request);
    }

    tracing::info!(event = "stdin_closed");
    ExitCode::SUCCESS
}

fn handle_request(synthesis_cache: &mut synthesis::SynthesisCache, req: TtsRequest) {
    let text_len = req.text.chars().count();
    tracing::debug!(
        event = "request_received",
        voice = req.voice.as_str(),
        speed = req.speed as f64,
        text_len
    );

    // Resolve the requested voice files before initializing eSpeak so the
    // host gets a deterministic missing-voice error when the model is wrong.
    let model_dir = Path::new(&req.model_dir);
    if let Err(message) = synthesis::validate_model_dir(model_dir) {
        tracing::warn!(
            event = "request_rejected",
            category = "invalid_request_payload",
            voice = req.voice.as_str(),
            speed = req.speed as f64,
            text_len,
            detail = message.as_str()
        );
        let _ = send_response(
            &TtsResponse::Error {
                message: format!("Invalid request payload: {}", message),
            },
            "error",
        );
        return;
    }

    let voice_paths = match synthesis::resolve_voice_paths(model_dir, &req.voice) {
        Ok(paths) => paths,
        Err(e) => {
            tracing::warn!(
                event = "request_rejected",
                category = "invalid_request_payload",
                voice = req.voice.as_str(),
                speed = req.speed as f64,
                text_len,
                detail = e.as_str()
            );
            let _ = send_response(
                &TtsResponse::Error {
                    message: format!("Invalid request payload: {}", e),
                },
                "error",
            );
            return;
        }
    };
    tracing::debug!(
        event = "voice_resolved",
        voice = req.voice.as_str(),
        model_path = voice_paths.model_path.display().to_string(),
        config_path = voice_paths.config_path.display().to_string()
    );

    // Synthesize
    match synthesis_cache.synthesize(&req.text, &voice_paths.config_path, req.speed) {
        Ok(result) => {
            // Convert i16 samples to bytes (PCM16 LE)
            let byte_len = (result.pcm16.len() * 2) as u32;

            // Send JSON header
            if !send_response(
                &TtsResponse::Audio {
                    byte_length: byte_len,
                    sample_rate: result.sample_rate,
                    channels: 1,
                },
                "audio",
            ) {
                return;
            }

            // Send raw PCM bytes immediately after
            let stdout = io::stdout();
            let mut out = stdout.lock();
            for sample in &result.pcm16 {
                let bytes = sample.to_le_bytes();
                if let Err(error) = out.write_all(&bytes) {
                    tracing::error!(
                        event = "stdout_write_failed",
                        stage = "audio_bytes",
                        error = error.to_string()
                    );
                    return;
                }
            }
            if let Err(error) = out.flush() {
                tracing::error!(
                    event = "stdout_flush_failed",
                    stage = "audio_bytes",
                    error = error.to_string()
                );
                return;
            }
            tracing::debug!(
                event = "request_succeeded",
                voice = req.voice.as_str(),
                speed = req.speed as f64,
                text_len,
                sample_rate = result.sample_rate as u64,
                byte_length = byte_len as u64
            );
        }
        Err(e) => {
            tracing::warn!(
                event = "request_failed",
                category = "synthesis_failed",
                voice = req.voice.as_str(),
                speed = req.speed as f64,
                text_len,
                detail = e.as_str()
            );
            let _ = send_response(
                &TtsResponse::Error {
                    message: format!("Synthesis failed: {}", e),
                },
                "error",
            );
        }
    }
}

fn parse_request(line: &str) -> Result<TtsRequest, String> {
    let request: TtsRequest = serde_json::from_str(line).map_err(|error| {
        if error.is_syntax() || error.is_eof() {
            format!("Invalid JSON request: {}", error)
        } else {
            format!("Invalid request payload: {}", error)
        }
    })?;

    request
        .validate()
        .map_err(|error| format!("Invalid request payload: {}", error))?;

    Ok(request)
}

fn discover_espeak_data_dir() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Cannot resolve current executable path: {}", error))?;
    let binary_dir = current_exe.parent().ok_or_else(|| {
        format!(
            "Cannot resolve binary directory from '{}'",
            current_exe.display()
        )
    })?;
    let candidate = binary_dir.join(ESPEAK_RUNTIME_DIR_NAME);
    discover_espeak_data_dir_in(&candidate)
}

fn discover_espeak_data_dir_in(candidate: &Path) -> Result<PathBuf, String> {
    synthesis::validate_espeak_data_dir(candidate)?;
    Ok(candidate.to_path_buf())
}

/// Send a JSON response to stdout followed by a newline.
fn send_response(response: &TtsResponse, response_type: &'static str) -> bool {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let json = serde_json::to_string(response).expect("Failed to serialize response");
    if let Err(error) = writeln!(out, "{}", json) {
        tracing::error!(
            event = "stdout_write_failed",
            stage = "response",
            response_type,
            error = error.to_string()
        );
        return false;
    }
    if let Err(error) = out.flush() {
        tracing::error!(
            event = "stdout_flush_failed",
            stage = "response",
            response_type,
            error = error.to_string()
        );
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{discover_espeak_data_dir_in, parse_request};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "lingopilot-tts-piper-{prefix}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    #[test]
    fn discover_espeak_data_dir_returns_candidate_when_runtime_layout_is_valid() {
        let root = unique_temp_dir("discover-valid");
        let candidate = root.join("espeak-runtime");
        fs::create_dir_all(candidate.join("espeak-ng-data")).expect("runtime layout");

        let resolved = discover_espeak_data_dir_in(&candidate)
            .expect("valid layout should resolve to the candidate path");
        assert_eq!(resolved, candidate);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_espeak_data_dir_rejects_missing_runtime() {
        let root = unique_temp_dir("discover-missing");
        let candidate = root.join("espeak-runtime");

        let error = discover_espeak_data_dir_in(&candidate)
            .expect_err("missing runtime should fail discovery");
        assert!(error.contains("eSpeak"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_request_rejects_semantically_invalid_payload_as_invalid_request_payload() {
        let error = parse_request(
            r#"{
                "text":"   ",
                "voice":"en_US-hfc_female-medium",
                "speed":1.0,
                "model_dir":"C:\\voices\\en_US-hfc_female-medium"
            }"#,
        )
        .expect_err("invalid semantic payload should fail");

        assert!(error.starts_with("Invalid request payload:"));
        assert!(error.contains("text must not be empty or whitespace"));
    }
}
