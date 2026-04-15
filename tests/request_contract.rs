use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const REAL_VOICE_DIR_ENV: &str = "PIPER_TTS_REAL_VOICE_DIR";
const REAL_VOICE_ID_ENV: &str = "PIPER_TTS_REAL_VOICE_ID";
const LOG_ENV: &str = "PIPER_TTS_LOG";

struct SidecarHarness {
    child: Child,
    stdout: BufReader<ChildStdout>,
    stderr: BufReader<ChildStderr>,
}

impl SidecarHarness {
    fn spawn() -> Self {
        Self::spawn_with_log_level(None)
    }

    fn spawn_with_log_level(level: Option<&str>) -> Self {
        let mut command = sidecar_command();
        if let Some(level) = level {
            command.env(LOG_ENV, level);
        }

        let mut child = command
            .arg("--espeak-data-dir")
            .arg(built_espeak_runtime_dir())
            .spawn()
            .expect("sidecar should start");

        let stdout = child.stdout.take().expect("stdout should be piped");
        let stderr = child.stderr.take().expect("stderr should be piped");

        Self {
            child,
            stdout: BufReader::new(stdout),
            stderr: BufReader::new(stderr),
        }
    }

    fn send_json(&mut self, value: Value) {
        let stdin = self.child.stdin.as_mut().expect("stdin should be piped");
        writeln!(stdin, "{value}").expect("request should be written");
        stdin.flush().expect("stdin should flush");
    }

    fn send_raw_line(&mut self, line: &str) {
        let stdin = self.child.stdin.as_mut().expect("stdin should be piped");
        writeln!(stdin, "{line}").expect("request should be written");
        stdin.flush().expect("stdin should flush");
    }

    fn read_json_line(&mut self) -> Value {
        let mut line = String::new();
        let bytes = self
            .stdout
            .read_line(&mut line)
            .expect("stdout should be readable");
        assert!(bytes > 0, "expected a JSON line from the sidecar");

        serde_json::from_str(line.trim_end()).expect("sidecar should emit valid JSON")
    }

    fn read_exact_bytes(&mut self, count: usize) -> Vec<u8> {
        let mut buffer = vec![0u8; count];
        self.stdout
            .read_exact(&mut buffer)
            .expect("stdout should contain the expected PCM payload");
        buffer
    }

    fn close_stdin(&mut self) {
        let _ = self.child.stdin.take();
    }

    fn read_remaining_stdout(&mut self) -> String {
        let mut remaining = String::new();
        self.stdout
            .read_to_string(&mut remaining)
            .expect("stdout should be readable until EOF");
        remaining
    }

    fn shutdown_and_collect_stderr(&mut self) -> String {
        self.close_stdin();
        let _ = self.child.wait();

        let mut stderr = String::new();
        self.stderr
            .read_to_string(&mut stderr)
            .expect("stderr should be valid UTF-8 text");
        stderr
    }
}

impl Drop for SidecarHarness {
    fn drop(&mut self) {
        self.close_stdin();
        let _ = self.child.wait();
    }
}

fn sidecar_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lingopilot-tts-piper"));
    command
        .env_remove(LOG_ENV)
        .env_remove("RUST_LOG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn unique_missing_path(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();

    std::env::temp_dir().join(format!(
        "lingopilot-tts-piper-{prefix}-{}-{nonce}",
        std::process::id()
    ))
}

struct TempDir {
    path: PathBuf,
}

struct RealVoiceFixture {
    model_dir: PathBuf,
    voice_id: String,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let path = unique_missing_path(prefix);
        fs::create_dir(&path).expect("temp dir should be created");
        Self { path }
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn built_espeak_runtime_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_lingopilot-tts-piper"))
        .parent()
        .expect("binary should have a parent directory")
        .join("espeak-runtime")
}

fn valid_semantic_request(model_dir: &Path) -> Value {
    valid_semantic_request_with_text(model_dir, "Hello from the new request contract")
}

fn valid_semantic_request_with_text(model_dir: &Path, text: &str) -> Value {
    json!({
        "text": text,
        "voice": "en_US-hfc_female-medium",
        "speed": 1.0,
        "model_dir": model_dir,
    })
}

fn real_voice_fixture_from_env() -> Option<RealVoiceFixture> {
    let model_dir = std::env::var_os(REAL_VOICE_DIR_ENV);
    let voice_id = std::env::var_os(REAL_VOICE_ID_ENV);

    match (model_dir, voice_id) {
        (None, None) => None,
        (Some(_), None) | (None, Some(_)) => {
            panic!(
                "real voice validation requires both {REAL_VOICE_DIR_ENV} and {REAL_VOICE_ID_ENV}"
            );
        }
        (Some(model_dir), Some(voice_id)) => Some(RealVoiceFixture {
            model_dir: PathBuf::from(model_dir),
            voice_id: voice_id
                .into_string()
                .expect("voice id env var should be valid UTF-8"),
        }),
    }
}

fn read_audio_response_and_payload(sidecar: &mut SidecarHarness) -> (Value, Vec<u8>) {
    let header = sidecar.read_json_line();
    assert_eq!(header["type"], "audio");

    let byte_length = header["byte_length"]
        .as_u64()
        .expect("audio response should contain byte_length") as usize;
    let payload = sidecar.read_exact_bytes(byte_length);
    assert_eq!(payload.len(), byte_length);

    (header, payload)
}

fn assert_stderr_is_plain_text(stderr: &str) {
    assert!(!stderr.contains('\0'), "stderr must not contain NUL bytes");
    assert!(
        !stderr.contains("{\"type\""),
        "stderr must not contain protocol JSON"
    );
}

#[test]
fn valid_startup_flag_emits_exactly_one_ready() {
    let mut sidecar = SidecarHarness::spawn();

    let ready = sidecar.read_json_line();
    assert_eq!(ready["type"], "ready");

    sidecar.close_stdin();
    let remaining = sidecar.read_remaining_stdout();
    assert!(remaining.is_empty(), "expected no extra stdout after ready");
}

#[test]
fn missing_startup_flag_exits_without_ready() {
    let output = sidecar_command()
        .output()
        .expect("sidecar should run to completion");

    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "expected no ready output on stdout"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Startup error: Missing required startup argument"));
    assert_stderr_is_plain_text(&stderr);
}

#[test]
fn invalid_startup_path_exits_without_ready() {
    let output = sidecar_command()
        .arg("--espeak-data-dir")
        .arg(unique_missing_path("missing-espeak-startup"))
        .output()
        .expect("sidecar should run to completion");

    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "expected no ready output on stdout"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Startup error: Cannot use eSpeak data dir"));
    assert_stderr_is_plain_text(&stderr);
}

#[test]
fn malformed_json_returns_error_and_process_stays_alive() {
    let mut sidecar = SidecarHarness::spawn();
    let valid_model_dir = TempDir::new("valid-follow-up-malformed-json");

    let ready = sidecar.read_json_line();
    assert_eq!(ready["type"], "ready");

    sidecar.send_raw_line(r#"{"text":"Hello","voice":"en_US-hfc_female-medium""#);

    let error = sidecar.read_json_line();
    assert_eq!(error["type"], "error");
    let message = error["message"]
        .as_str()
        .expect("error response should contain a message");
    assert!(message.contains("Invalid JSON request:"));

    sidecar.send_json(valid_semantic_request(valid_model_dir.path()));

    let follow_up = sidecar.read_json_line();
    assert_eq!(follow_up["type"], "error");
    let follow_up_message = follow_up["message"]
        .as_str()
        .expect("follow-up error should contain a message");
    assert!(!follow_up_message.contains("Invalid JSON request"));
}

#[test]
fn request_with_legacy_espeak_data_dir_is_rejected_and_process_stays_alive() {
    let mut sidecar = SidecarHarness::spawn();
    let valid_model_dir = TempDir::new("valid-follow-up-espeak");

    let ready = sidecar.read_json_line();
    assert_eq!(ready["type"], "ready");

    sidecar.send_json(json!({
        "text": "Hello",
        "voice": "en_US-hfc_female-medium",
        "speed": 1.0,
        "model_dir": "unused",
        "espeak_data_dir": "unused"
    }));

    let error = sidecar.read_json_line();
    assert_eq!(error["type"], "error");
    let message = error["message"]
        .as_str()
        .expect("error response should contain a message");
    assert!(message.contains("unknown field `espeak_data_dir`"));

    sidecar.send_json(valid_semantic_request(valid_model_dir.path()));

    let follow_up = sidecar.read_json_line();
    assert_eq!(follow_up["type"], "error");
    let follow_up_message = follow_up["message"]
        .as_str()
        .expect("follow-up error should contain a message");
    assert!(!follow_up_message.contains("Invalid JSON request"));
}

#[test]
fn request_with_language_is_rejected_and_process_stays_alive() {
    let mut sidecar = SidecarHarness::spawn();
    let valid_model_dir = TempDir::new("valid-follow-up-language");

    let ready = sidecar.read_json_line();
    assert_eq!(ready["type"], "ready");

    sidecar.send_json(json!({
        "text": "Hello",
        "language": "en",
        "voice": "en_US-hfc_female-medium",
        "speed": 1.0,
        "model_dir": "unused"
    }));

    let error = sidecar.read_json_line();
    assert_eq!(error["type"], "error");
    let message = error["message"]
        .as_str()
        .expect("error response should contain a message");
    assert!(message.contains("unknown field `language`"));

    sidecar.send_json(valid_semantic_request(valid_model_dir.path()));

    let follow_up = sidecar.read_json_line();
    assert_eq!(follow_up["type"], "error");
    let follow_up_message = follow_up["message"]
        .as_str()
        .expect("follow-up error should contain a message");
    assert!(!follow_up_message.contains("Invalid JSON request"));
}

#[test]
fn valid_semantic_request_passes_contract_validation() {
    let mut sidecar = SidecarHarness::spawn();
    let valid_model_dir = TempDir::new("valid-contract-shape");

    let ready = sidecar.read_json_line();
    assert_eq!(ready["type"], "ready");

    sidecar.send_json(valid_semantic_request(valid_model_dir.path()));

    let response = sidecar.read_json_line();
    assert_eq!(response["type"], "error");
    let message = response["message"]
        .as_str()
        .expect("error response should contain a message");

    assert!(message.contains("Invalid request payload:"));
    assert!(!message.contains("Invalid JSON request:"));
    assert!(!message.contains("unknown field"));
    assert!(!message.contains("missing field"));
}

#[test]
fn unicode_and_escaped_newlines_remain_valid_until_missing_voice_resolution() {
    let mut sidecar = SidecarHarness::spawn();
    let model_dir = TempDir::new("unicode-escaped-newlines");

    let ready = sidecar.read_json_line();
    assert_eq!(ready["type"], "ready");

    sidecar.send_json(json!({
        "text": "Olá, 世界 👋\nSecond line stays inside the JSON string.",
        "voice": "en_US-hfc_female-medium",
        "speed": 1.0,
        "model_dir": model_dir.path(),
    }));

    let error = sidecar.read_json_line();
    assert_eq!(error["type"], "error");
    let message = error["message"]
        .as_str()
        .expect("error response should contain a message");
    assert!(message.contains("Invalid request payload:"));
    assert!(message.contains("en_US-hfc_female-medium.onnx"));
    assert!(!message.contains("Invalid JSON request:"));
    assert!(!message.contains("text must"));
}

#[test]
fn missing_voice_returns_error_before_synthesis_and_process_stays_alive() {
    let mut sidecar = SidecarHarness::spawn();
    let valid_model_dir = TempDir::new("valid-follow-up-missing-voice");

    let ready = sidecar.read_json_line();
    assert_eq!(ready["type"], "ready");

    let model_dir = TempDir::new("missing-voice");
    fs::write(model_dir.path().join("other-voice.onnx"), b"model")
        .expect("model should be created");
    fs::write(model_dir.path().join("other-voice.onnx.json"), b"{}")
        .expect("config should be created");

    sidecar.send_json(json!({
        "text": "Hello",
        "voice": "missing-voice",
        "speed": 1.0,
        "model_dir": model_dir.path(),
    }));

    let error = sidecar.read_json_line();
    assert_eq!(error["type"], "error");
    let message = error["message"]
        .as_str()
        .expect("error response should contain a message");
    assert!(message.contains("Invalid request payload:"));
    assert!(message.contains("missing-voice"));
    assert!(message.contains("missing-voice.onnx"));
    assert!(!message.contains("eSpeak init failed"));

    sidecar.send_json(valid_semantic_request(valid_model_dir.path()));

    let follow_up = sidecar.read_json_line();
    assert_eq!(follow_up["type"], "error");
    let follow_up_message = follow_up["message"]
        .as_str()
        .expect("follow-up error should contain a message");
    assert!(!follow_up_message.contains("Invalid JSON request"));
}

#[test]
fn process_handles_multiple_requests_after_startup_validation() {
    let mut sidecar = SidecarHarness::spawn();
    let first_model_dir = TempDir::new("valid-first-request");
    let second_model_dir = TempDir::new("valid-second-request");

    let ready = sidecar.read_json_line();
    assert_eq!(ready["type"], "ready");

    sidecar.send_json(valid_semantic_request(first_model_dir.path()));
    let first = sidecar.read_json_line();
    assert_eq!(first["type"], "error");
    let first_message = first["message"]
        .as_str()
        .expect("first response should contain a message");
    assert!(!first_message.contains("Invalid JSON request"));

    sidecar.send_json(valid_semantic_request(second_model_dir.path()));
    let second = sidecar.read_json_line();
    assert_eq!(second["type"], "error");
    let second_message = second["message"]
        .as_str()
        .expect("second response should contain a message");
    assert!(!second_message.contains("Invalid JSON request"));
}

#[test]
fn invalid_semantic_payload_returns_error_and_process_stays_alive() {
    let mut sidecar = SidecarHarness::spawn();
    let valid_model_dir = TempDir::new("valid-after-semantic-errors");
    let invalid_file_dir = TempDir::new("invalid-model-file");
    let invalid_file_path = invalid_file_dir.path().join("voice.onnx");
    fs::write(&invalid_file_path, b"model").expect("file should be created");
    let oversized_text = "a".repeat(8193);

    let ready = sidecar.read_json_line();
    assert_eq!(ready["type"], "ready");

    let invalid_requests = vec![
        (
            json!({
                "text": "",
                "voice": "en_US-hfc_female-medium",
                "speed": 1.0,
                "model_dir": valid_model_dir.path(),
            }),
            "text must not be empty or whitespace",
        ),
        (
            json!({
                "text": "   \n\t  ",
                "voice": "en_US-hfc_female-medium",
                "speed": 1.0,
                "model_dir": valid_model_dir.path(),
            }),
            "text must not be empty or whitespace",
        ),
        (
            json!({
                "text": oversized_text,
                "voice": "en_US-hfc_female-medium",
                "speed": 1.0,
                "model_dir": valid_model_dir.path(),
            }),
            "text must be at most 8192 characters",
        ),
        (
            json!({
                "text": "Hello",
                "voice": "en_US-hfc_female-medium",
                "speed": 0.49,
                "model_dir": valid_model_dir.path(),
            }),
            "speed must be a finite number between 0.5 and 5.5",
        ),
        (
            json!({
                "text": "Hello",
                "voice": "en_US-hfc_female-medium",
                "speed": 5.51,
                "model_dir": valid_model_dir.path(),
            }),
            "speed must be a finite number between 0.5 and 5.5",
        ),
        (
            json!({
                "text": "Hello",
                "voice": "en_US-hfc_female-medium",
                "speed": 1.0,
                "model_dir": "relative-model-dir",
            }),
            "path must be absolute",
        ),
        (
            json!({
                "text": "Hello",
                "voice": "en_US-hfc_female-medium",
                "speed": 1.0,
                "model_dir": unique_missing_path("missing-model-dir"),
            }),
            "path does not exist",
        ),
        (
            json!({
                "text": "Hello",
                "voice": "en_US-hfc_female-medium",
                "speed": 1.0,
                "model_dir": invalid_file_path,
            }),
            "path is not a directory",
        ),
    ];

    for (request, expected_fragment) in invalid_requests {
        sidecar.send_json(request);

        let error = sidecar.read_json_line();
        assert_eq!(error["type"], "error");
        let message = error["message"]
            .as_str()
            .expect("error response should contain a message");
        assert!(message.contains("Invalid request payload:"));
        assert!(message.contains(expected_fragment));

        sidecar.send_json(valid_semantic_request(valid_model_dir.path()));
        let follow_up = sidecar.read_json_line();
        assert_eq!(follow_up["type"], "error");
        let follow_up_message = follow_up["message"]
            .as_str()
            .expect("follow-up error should contain a message");
        assert!(!follow_up_message.contains("Invalid JSON request"));
    }
}

#[test]
fn process_can_restart_after_invalid_startup_and_then_emit_ready() {
    let invalid_output = sidecar_command()
        .arg("--espeak-data-dir")
        .arg(unique_missing_path("restart-invalid-espeak"))
        .output()
        .expect("invalid startup should complete");
    assert!(!invalid_output.status.success());

    let mut sidecar = SidecarHarness::spawn();
    let ready = sidecar.read_json_line();
    assert_eq!(ready["type"], "ready");
}

#[test]
fn startup_warn_logging_keeps_stdout_protocol_only_and_stderr_text_only() {
    let output = sidecar_command()
        .env(LOG_ENV, "warn")
        .arg("--espeak-data-dir")
        .arg(built_espeak_runtime_dir())
        .output()
        .expect("sidecar should run to completion");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8 JSON only");
    assert_eq!(
        stdout.lines().count(),
        1,
        "expected exactly one stdout line"
    );
    let ready: Value = serde_json::from_str(stdout.trim_end()).expect("ready should be valid JSON");
    assert_eq!(ready["type"], "ready");

    let stderr = String::from_utf8(output.stderr).expect("stderr should be valid UTF-8 text");
    assert_stderr_is_plain_text(&stderr);
}

#[test]
fn startup_debug_logging_keeps_stdout_protocol_only_and_stderr_plain_text() {
    let output = sidecar_command()
        .env(LOG_ENV, "debug")
        .arg("--espeak-data-dir")
        .arg(built_espeak_runtime_dir())
        .output()
        .expect("sidecar should run to completion");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8 JSON only");
    assert_eq!(
        stdout.lines().count(),
        1,
        "expected exactly one stdout line"
    );
    let ready: Value = serde_json::from_str(stdout.trim_end()).expect("ready should be valid JSON");
    assert_eq!(ready["type"], "ready");

    let stderr = String::from_utf8(output.stderr).expect("stderr should be valid UTF-8 text");
    assert!(stderr.contains("level=INFO event=startup"));
    assert!(stderr.contains("level=INFO event=stdin_closed"));
    assert_stderr_is_plain_text(&stderr);
}

#[test]
fn malformed_json_under_debug_logging_keeps_stdout_json_only_and_stderr_text_only() {
    let mut sidecar = SidecarHarness::spawn_with_log_level(Some("debug"));

    let ready = sidecar.read_json_line();
    assert_eq!(ready["type"], "ready");

    sidecar.send_raw_line(r#"{"text":"Hello","voice":"en_US-hfc_female-medium""#);

    let error = sidecar.read_json_line();
    assert_eq!(error["type"], "error");
    let message = error["message"]
        .as_str()
        .expect("error response should contain a message");
    assert!(message.contains("Invalid JSON request:"));

    sidecar.close_stdin();
    let remaining_stdout = sidecar.read_remaining_stdout();
    assert!(
        remaining_stdout.is_empty(),
        "stdout must not contain log output"
    );

    let stderr = sidecar.shutdown_and_collect_stderr();
    assert!(stderr.contains("level=WARN event=request_rejected"));
    assert!(stderr.contains("category=invalid_json"));
    assert_stderr_is_plain_text(&stderr);
}

#[test]
fn missing_voice_under_debug_logging_uses_payload_error_prefix_and_text_stderr() {
    let mut sidecar = SidecarHarness::spawn_with_log_level(Some("debug"));

    let ready = sidecar.read_json_line();
    assert_eq!(ready["type"], "ready");

    let model_dir = TempDir::new("missing-voice-debug");
    fs::write(model_dir.path().join("other-voice.onnx"), b"model")
        .expect("model should be created");
    fs::write(model_dir.path().join("other-voice.onnx.json"), b"{}")
        .expect("config should be created");

    sidecar.send_json(json!({
        "text": "Hello",
        "voice": "missing-voice",
        "speed": 1.0,
        "model_dir": model_dir.path(),
    }));

    let error = sidecar.read_json_line();
    assert_eq!(error["type"], "error");
    let message = error["message"]
        .as_str()
        .expect("error response should contain a message");
    assert!(message.starts_with("Invalid request payload:"));
    assert!(message.contains("missing-voice"));

    sidecar.close_stdin();
    let remaining_stdout = sidecar.read_remaining_stdout();
    assert!(
        remaining_stdout.is_empty(),
        "stdout must not contain log output"
    );

    let stderr = sidecar.shutdown_and_collect_stderr();
    assert!(stderr.contains("level=WARN event=request_rejected"));
    assert!(stderr.contains("category=invalid_request_payload"));
    assert_stderr_is_plain_text(&stderr);
}

#[test]
fn real_voice_fixture_allows_two_successive_audio_responses_when_configured() {
    let Some(fixture) = real_voice_fixture_from_env() else {
        eprintln!(
            "skipping real voice validation because {} and {} are not set",
            REAL_VOICE_DIR_ENV, REAL_VOICE_ID_ENV
        );
        return;
    };

    let mut sidecar = SidecarHarness::spawn();

    let ready = sidecar.read_json_line();
    assert_eq!(ready["type"], "ready");

    let request = |text: &str| {
        json!({
            "text": text,
            "voice": fixture.voice_id,
            "speed": 1.0,
            "model_dir": fixture.model_dir,
        })
    };

    sidecar.send_json(request("Real voice fixture validation request one."));
    let (first_header, first_payload) = read_audio_response_and_payload(&mut sidecar);
    assert_eq!(first_header["channels"], 1);
    assert!(
        first_header["sample_rate"]
            .as_u64()
            .expect("sample_rate should be present")
            > 0
    );
    assert!(
        !first_payload.is_empty(),
        "audio payload should not be empty"
    );

    sidecar.send_json(request("Real voice fixture validation request two."));
    let (second_header, second_payload) = read_audio_response_and_payload(&mut sidecar);
    assert_eq!(second_header["channels"], 1);
    assert!(
        second_header["sample_rate"]
            .as_u64()
            .expect("sample_rate should be present")
            > 0
    );
    assert!(
        !second_payload.is_empty(),
        "audio payload should not be empty"
    );
}
