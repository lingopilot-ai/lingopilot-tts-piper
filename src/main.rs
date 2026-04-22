mod phonemize;
mod protocol;
mod synthesis;

use std::fmt;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use protocol::{
    PhonemizeRequest, SidecarRequest, SidecarResponse, SynthesizeRequest, CHANNELS, ENCODING,
    SAMPLE_RATE_HZ, SUPPORTED_OPS,
};
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

    if !emit_ready() {
        return ExitCode::FAILURE;
    }

    let mut synthesis_cache = synthesis::SynthesisCache::new();

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

        match parse_request(&line) {
            Ok(request) => handle_request(&mut synthesis_cache, request),
            Err((id, kind, message)) => {
                tracing::warn!(
                    event = "request_rejected",
                    kind = kind,
                    line_len = line.chars().count(),
                    detail = message.as_str()
                );
                send_error(id.as_deref(), kind, &message);
            }
        }
    }

    tracing::info!(event = "stdin_closed");
    ExitCode::SUCCESS
}

fn emit_ready() -> bool {
    let ops: &[&str] = &SUPPORTED_OPS;
    send_response(&SidecarResponse::Ready {
        version: VERSION,
        sample_rate: SAMPLE_RATE_HZ,
        channels: CHANNELS,
        encoding: ENCODING,
        ops,
    })
}

fn parse_request(line: &str) -> Result<SidecarRequest, (Option<String>, &'static str, String)> {
    let id = extract_id_from_raw(line);
    let request: SidecarRequest = serde_json::from_str(line).map_err(|error| {
        (
            id.clone(),
            "bad_request",
            format!("Invalid request: {}", error),
        )
    })?;
    request
        .validate()
        .map_err(|error| (Some(request.id().to_string()), "bad_request", error))?;
    Ok(request)
}

fn extract_id_from_raw(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    value.get("id")?.as_str().map(|s| s.to_string())
}

fn handle_request(cache: &mut synthesis::SynthesisCache, request: SidecarRequest) {
    match request {
        SidecarRequest::Synthesize(r) => handle_synthesize(cache, r),
        SidecarRequest::Phonemize(r) => handle_phonemize(r),
    }
}

fn handle_synthesize(cache: &mut synthesis::SynthesisCache, req: SynthesizeRequest) {
    let text_len = req.text.chars().count();
    tracing::debug!(
        event = "request_received",
        op = "synthesize",
        id = req.id.as_str(),
        speed = req.speed as f64,
        text_len
    );

    let model_path = Path::new(&req.voice_model_path);
    if let Err(message) = synthesis::validate_voice_file(model_path, "voice_model_path") {
        send_error(Some(&req.id), "voice_not_found", &message);
        return;
    }
    let config_path = Path::new(&req.voice_config_path);
    if let Err(message) = synthesis::validate_voice_file(config_path, "voice_config_path") {
        send_error(Some(&req.id), "voice_not_found", &message);
        return;
    }

    let speed = req.clamped_speed();

    match cache.synthesize(&req.text, config_path, speed) {
        Ok(result) => {
            let byte_len = (result.pcm16.len() * 2) as u32;

            if !send_response(&SidecarResponse::Audio {
                id: &req.id,
                bytes: byte_len,
                sample_rate: SAMPLE_RATE_HZ,
                channels: CHANNELS,
            }) {
                return;
            }

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

            let _ = send_response(&SidecarResponse::Done { id: &req.id });
            tracing::debug!(
                event = "request_succeeded",
                op = "synthesize",
                id = req.id.as_str(),
                text_len,
                byte_length = byte_len as u64
            );
        }
        Err(e) => {
            tracing::warn!(
                event = "request_failed",
                op = "synthesize",
                id = req.id.as_str(),
                text_len,
                detail = e.as_str()
            );
            send_error(Some(&req.id), "synthesis_failed", &e);
        }
    }
}

fn handle_phonemize(req: PhonemizeRequest) {
    tracing::debug!(
        event = "request_received",
        op = "phonemize",
        id = req.id.as_str(),
        language = req.language.as_str(),
        text_len = req.text.chars().count()
    );

    match phonemize::phonemize(&req.text, &req.language) {
        Ok(phonemes) => {
            let _ = send_response(&SidecarResponse::Phonemes {
                id: &req.id,
                phonemes: &phonemes,
            });
            tracing::debug!(
                event = "request_succeeded",
                op = "phonemize",
                id = req.id.as_str(),
                phoneme_len = phonemes.chars().count()
            );
        }
        Err(message) => {
            tracing::warn!(
                event = "request_failed",
                op = "phonemize",
                id = req.id.as_str(),
                detail = message.as_str()
            );
            send_error(Some(&req.id), "phonemize_failed", &message);
        }
    }
}

fn send_error(id: Option<&str>, kind: &str, message: &str) {
    let _ = send_response(&SidecarResponse::Error {
        id,
        kind,
        message,
    });
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

fn send_response(response: &SidecarResponse<'_>) -> bool {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let json = serde_json::to_string(response).expect("Failed to serialize response");
    if let Err(error) = writeln!(out, "{}", json) {
        tracing::error!(
            event = "stdout_write_failed",
            stage = "response",
            error = error.to_string()
        );
        return false;
    }
    if let Err(error) = out.flush() {
        tracing::error!(
            event = "stdout_flush_failed",
            stage = "response",
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
    fn parse_request_rejects_malformed_json_with_bad_request_kind() {
        let (_id, kind, message) = parse_request(r#"{"op":"synthesize"#)
            .expect_err("malformed JSON should fail");
        assert_eq!(kind, "bad_request");
        assert!(message.starts_with("Invalid request:"));
    }

    #[test]
    fn parse_request_rejects_unknown_op() {
        let (_id, kind, _message) = parse_request(r#"{"op":"cancel","id":"r1"}"#)
            .expect_err("cancel is not supported");
        assert_eq!(kind, "bad_request");
    }

    #[test]
    fn parse_request_extracts_id_from_semantically_invalid_payload() {
        let (id, kind, _message) =
            parse_request(r#"{"op":"synthesize","id":"req-42","text":"","voice_model_path":"a","voice_config_path":"b"}"#)
                .expect_err("empty text should fail validation");
        assert_eq!(id.as_deref(), Some("req-42"));
        assert_eq!(kind, "bad_request");
    }
}
