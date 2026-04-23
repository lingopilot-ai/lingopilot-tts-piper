use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const LOG_ENV: &str = "PIPER_TTS_LOG";
const REAL_VOICE_DIR_ENV: &str = "PIPER_TTS_REAL_VOICE_DIR";
const REAL_VOICE_ID_ENV: &str = "PIPER_TTS_REAL_VOICE_ID";

struct SidecarHarness {
    child: Child,
    stdout: BufReader<ChildStdout>,
    stderr: BufReader<ChildStderr>,
}

impl SidecarHarness {
    fn spawn() -> Self {
        let mut child = sidecar_command().spawn().expect("sidecar should start");
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

fn assert_stderr_is_plain_text(stderr: &str) {
    assert!(!stderr.contains('\0'), "stderr must not contain NUL bytes");
    assert!(
        !stderr.contains(r#""op":"#),
        "stderr must not contain protocol JSON"
    );
}

fn ready_line() -> Value {
    let mut sidecar = SidecarHarness::spawn();
    let ready = sidecar.read_json_line();
    sidecar.close_stdin();
    ready
}

#[test]
fn ready_line_matches_directive_shape_exactly() {
    let output = sidecar_command()
        .output()
        .expect("sidecar should run to completion");
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    let line = stdout.lines().next().expect("ready line must exist");
    // Empty stdin closes the sidecar, so there is exactly one ready line.
    assert_eq!(stdout.lines().count(), 1);

    let version = env!("CARGO_PKG_VERSION");
    let expected = format!(
        r#"{{"op":"ready","version":"{version}","sample_rate":22050,"channels":1,"encoding":"pcm16le","ops":["synthesize","phonemize"]}}"#
    );
    assert_eq!(line, expected);
}

#[test]
fn ready_uses_op_discriminator_not_type() {
    let ready = ready_line();
    assert_eq!(ready["op"], "ready");
    assert!(ready.get("type").is_none(), "ready must not contain 'type'");
    assert_eq!(ready["sample_rate"], 22050);
    assert_eq!(ready["channels"], 1);
    assert_eq!(ready["encoding"], "pcm16le");
    assert_eq!(ready["ops"], json!(["synthesize", "phonemize"]));
}

#[test]
fn unexpected_startup_argument_exits_without_ready() {
    let output = sidecar_command()
        .arg("--unexpected-flag")
        .output()
        .expect("sidecar should run to completion");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Startup error:"));
    assert_stderr_is_plain_text(&stderr);
}

#[test]
fn malformed_json_returns_bad_request_error_and_process_stays_alive() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    sidecar.send_raw_line(r#"{"op":"synthesize","id":"r1""#);
    let error = sidecar.read_json_line();
    assert_eq!(error["op"], "error");
    assert_eq!(error["kind"], "bad_request");
    assert!(error["message"].as_str().unwrap().starts_with("Invalid request:"));

    // process is still alive: second malformed request also returns error
    sidecar.send_raw_line("{not json");
    let error2 = sidecar.read_json_line();
    assert_eq!(error2["op"], "error");
    assert_eq!(error2["kind"], "bad_request");
}

#[test]
fn unknown_op_returns_bad_request() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    sidecar.send_json(json!({"op": "cancel", "id": "r1"}));
    let error = sidecar.read_json_line();
    assert_eq!(error["op"], "error");
    assert_eq!(error["kind"], "bad_request");
    assert_eq!(error["id"], "r1");
}

#[test]
fn audio_chunk_op_is_rejected_as_bad_request() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    sidecar.send_json(json!({"op": "audio_chunk", "id": "r1"}));
    let error = sidecar.read_json_line();
    assert_eq!(error["op"], "error");
    assert_eq!(error["kind"], "bad_request");
}

#[test]
fn synthesize_with_missing_model_returns_voice_not_found() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    let missing = unique_missing_path("missing-model").join("voice.onnx");
    sidecar.send_json(json!({
        "op": "synthesize",
        "id": "r1",
        "text": "hello",
        "voice_model_path": missing,
        "voice_config_path": missing,
    }));
    let error = sidecar.read_json_line();
    assert_eq!(error["op"], "error");
    assert_eq!(error["kind"], "voice_not_found");
    assert_eq!(error["id"], "r1");
}

#[test]
fn synthesize_rejects_relative_voice_paths_as_voice_not_found() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    sidecar.send_json(json!({
        "op": "synthesize",
        "id": "r1",
        "text": "hi",
        "voice_model_path": "relative.onnx",
        "voice_config_path": "relative.onnx.json",
    }));
    let error = sidecar.read_json_line();
    assert_eq!(error["op"], "error");
    assert_eq!(error["kind"], "voice_not_found");
}

#[test]
fn synthesize_rejects_id_longer_than_128_bytes_as_bad_request() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    sidecar.send_json(json!({
        "op": "synthesize",
        "id": "x".repeat(129),
        "text": "hi",
        "voice_model_path": "a",
        "voice_config_path": "b",
    }));
    let error = sidecar.read_json_line();
    assert_eq!(error["op"], "error");
    assert_eq!(error["kind"], "bad_request");
    assert!(error["message"]
        .as_str()
        .unwrap()
        .contains("128 bytes"));
}

#[test]
fn legacy_synthesize_shape_is_rejected_and_process_stays_alive() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();
    let temp = TempDir::new("legacy-shape");

    // Old shape: no `op`, uses `voice` + `model_dir`. Must be rejected.
    sidecar.send_json(json!({
        "text": "hi",
        "voice": "en_US-hfc_female-medium",
        "speed": 1.0,
        "model_dir": temp.path(),
    }));
    let error = sidecar.read_json_line();
    assert_eq!(error["op"], "error");
    assert_eq!(error["kind"], "bad_request");
}

#[test]
fn phonemize_returns_phonemes_line_for_simple_english_text() {
    let mut sidecar = SidecarHarness::spawn();
    let ready = sidecar.read_json_line();
    assert_eq!(ready["op"], "ready");

    sidecar.send_json(json!({
        "op": "phonemize",
        "id": "p1",
        "text": "hello",
        "language": "en",
    }));
    let response = sidecar.read_json_line();
    assert_eq!(response["op"], "phonemes");
    assert_eq!(response["id"], "p1");
    let phonemes = response["phonemes"]
        .as_str()
        .expect("phonemes field must be a string");
    assert!(!phonemes.is_empty(), "phonemes must be non-empty for 'hello'");
    let words = response["words"]
        .as_array()
        .expect("words field must be an array");
    assert_eq!(words.len(), 1);
    assert_eq!(words[0]["text"], "hello");
    assert!(!words[0]["phonemes"].as_str().unwrap().is_empty());
}

#[test]
fn phonemize_response_includes_words_array_for_multi_word_english() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    sidecar.send_json(json!({
        "op": "phonemize",
        "id": "p1",
        "text": "I would like a cup of coffee",
        "language": "en-US",
    }));
    let response = sidecar.read_json_line();
    assert_eq!(response["op"], "phonemes");
    let words = response["words"]
        .as_array()
        .expect("words must be an array");
    assert_eq!(words.len(), 7);
    let reconstructed: Vec<String> = words
        .iter()
        .map(|w| w["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(reconstructed.join(" "), "I would like a cup of coffee");
    for w in words {
        assert!(!w["phonemes"].as_str().unwrap().is_empty());
    }
}

#[test]
fn phonemize_response_has_empty_words_for_empty_text() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    sidecar.send_json(json!({
        "op": "phonemize",
        "id": "p1",
        "text": "",
        "language": "en-US",
    }));
    let response = sidecar.read_json_line();
    assert_eq!(response["op"], "phonemes");
    assert_eq!(response["id"], "p1");
    assert_eq!(response["phonemes"], "");
    assert_eq!(response["words"], json!([]));
}

#[test]
fn phonemize_response_has_empty_words_for_whitespace_only_text() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    sidecar.send_json(json!({
        "op": "phonemize",
        "id": "p1",
        "text": "   \t  ",
        "language": "en-US",
    }));
    let response = sidecar.read_json_line();
    assert_eq!(response["op"], "phonemes");
    assert_eq!(response["phonemes"], "");
    assert_eq!(response["words"], json!([]));
}

#[test]
fn phonemize_response_has_empty_words_for_punct_only_text() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    sidecar.send_json(json!({
        "op": "phonemize",
        "id": "p1",
        "text": "... !! ??",
        "language": "en-US",
    }));
    let response = sidecar.read_json_line();
    assert_eq!(response["op"], "phonemes");
    assert_eq!(response["phonemes"], "");
    assert_eq!(response["words"], json!([]));
}

#[test]
fn phonemize_rejects_empty_language() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    sidecar.send_json(json!({
        "op": "phonemize",
        "id": "p1",
        "text": "hello",
        "language": "",
    }));
    let error = sidecar.read_json_line();
    assert_eq!(error["op"], "error");
    assert_eq!(error["kind"], "bad_request");
    assert_eq!(error["id"], "p1");
}

#[test]
fn phonemize_returns_unsupported_language_for_unknown_bcp47() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    sidecar.send_json(json!({
        "op": "phonemize",
        "id": "p1",
        "text": "hello",
        "language": "zz-ZZ",
    }));
    let error = sidecar.read_json_line();
    assert_eq!(error["op"], "error");
    assert_eq!(error["kind"], "unsupported_language");
    assert_eq!(error["id"], "p1");
    assert!(error["message"].as_str().unwrap().contains("zz-ZZ"));

    // Process stays alive for a subsequent valid request.
    sidecar.send_json(json!({
        "op": "phonemize",
        "id": "p2",
        "text": "hello",
        "language": "en-US",
    }));
    let response = sidecar.read_json_line();
    assert_eq!(response["op"], "phonemes");
    assert_eq!(response["id"], "p2");
}

#[test]
fn phonemize_top_level_string_matches_v0_1_5_baseline_fixture() {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/phoneme_baseline_v0_1_5.json");
    let raw = fs::read_to_string(&fixture_path).expect("baseline fixture must exist");
    let fixture: Value = serde_json::from_str(&raw).expect("baseline must be JSON");
    let entries = fixture["entries"]
        .as_array()
        .expect("baseline.entries must be an array");
    let language = fixture["language"].as_str().unwrap_or("en-us");

    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    for (i, entry) in entries.iter().enumerate() {
        let text = entry["text"].as_str().unwrap();
        let expected = entry["phonemes"].as_str().unwrap();

        sidecar.send_json(json!({
            "op": "phonemize",
            "id": format!("baseline-{i}"),
            "text": text,
            "language": language,
        }));
        let response = sidecar.read_json_line();
        assert_eq!(response["op"], "phonemes", "entry {i}: {text}");
        assert_eq!(
            response["phonemes"].as_str().unwrap(),
            expected,
            "byte-identity regression for entry {i}: {text:?}"
        );
    }
}

#[test]
fn phonemize_output_is_deterministic_across_iterations() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();

    let text = "I would like a cup of coffee";
    let mut first: Option<(String, Value)> = None;
    for i in 0..10 {
        sidecar.send_json(json!({
            "op": "phonemize",
            "id": format!("det-{i}"),
            "text": text,
            "language": "en-US",
        }));
        let response = sidecar.read_json_line();
        assert_eq!(response["op"], "phonemes");
        let phonemes = response["phonemes"].as_str().unwrap().to_string();
        let words = response["words"].clone();
        match &first {
            None => first = Some((phonemes, words)),
            Some((prev_phonemes, prev_words)) => {
                assert_eq!(
                    &phonemes, prev_phonemes,
                    "phonemize should be deterministic across iterations"
                );
                assert_eq!(
                    &words, prev_words,
                    "words should be deterministic across iterations"
                );
            }
        }
    }
}

#[test]
fn stdin_close_exits_cleanly() {
    let mut sidecar = SidecarHarness::spawn();
    let _ready = sidecar.read_json_line();
    sidecar.close_stdin();
    let remaining = sidecar.read_remaining_stdout();
    assert!(remaining.is_empty(), "no extra stdout after ready+close");
}

#[test]
fn startup_warn_logging_keeps_stdout_protocol_only_and_stderr_plain_text() {
    let output = sidecar_command()
        .env(LOG_ENV, "warn")
        .output()
        .expect("sidecar should run to completion");
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    assert_eq!(stdout.lines().count(), 1);
    let ready: Value = serde_json::from_str(stdout.trim_end()).expect("ready JSON");
    assert_eq!(ready["op"], "ready");

    let stderr = String::from_utf8(output.stderr).expect("stderr UTF-8");
    assert_stderr_is_plain_text(&stderr);
}

#[test]
fn synthesize_with_real_voice_emits_audio_payload_and_done_when_configured() {
    let Some((model_dir, voice_id)) = real_voice_fixture_from_env() else {
        eprintln!("skipping: set {REAL_VOICE_DIR_ENV}+{REAL_VOICE_ID_ENV} to enable");
        return;
    };

    let model_path = model_dir.join(format!("{voice_id}.onnx"));
    let config_path = model_dir.join(format!("{voice_id}.onnx.json"));

    let mut sidecar = SidecarHarness::spawn();
    let ready = sidecar.read_json_line();
    assert_eq!(ready["op"], "ready");

    sidecar.send_json(json!({
        "op": "synthesize",
        "id": "real-voice-1",
        "text": "Release readiness validation.",
        "voice_model_path": model_path,
        "voice_config_path": config_path,
    }));

    let audio = sidecar.read_json_line();
    assert_eq!(audio["op"], "audio");
    assert_eq!(audio["id"], "real-voice-1");
    assert_eq!(audio["sample_rate"], 22050);
    assert_eq!(audio["channels"], 1);
    let bytes_len = audio["bytes"]
        .as_u64()
        .expect("bytes field must be u64") as usize;
    assert!(bytes_len > 0);

    let _payload = sidecar.read_exact_bytes(bytes_len);

    let done = sidecar.read_json_line();
    assert_eq!(done["op"], "done");
    assert_eq!(done["id"], "real-voice-1");
}

fn real_voice_fixture_from_env() -> Option<(PathBuf, String)> {
    let dir = std::env::var_os(REAL_VOICE_DIR_ENV)?;
    let id = std::env::var_os(REAL_VOICE_ID_ENV)?;
    Some((PathBuf::from(dir), id.into_string().ok()?))
}
