// Integration contract tests for op:ping / op:pong (H-01).
//
// Pattern mirrors tests/request_contract.rs: spawn the binary, consume the
// `ready` line, then exercise the ping path.  Two tests:
//
//   ping_after_ready_returns_pong
//       Sends a single ping immediately after ready and asserts the pong line
//       is byte-exact `{"op":"pong","id":"h1"}`.
//
//   ping_between_requests_preserves_framing
//       Uses two phonemize requests (no PCM, no env gate) to bracket a ping.
//       The ping is written STRICTLY after the first response is fully consumed
//       and BEFORE the second request is written.  ADR §5.1 forbids sending a
//       ping between the `audio` and `done` lines of a synthesis; this test
//       enforces that constraint by never doing synthesis at all — phonemize
//       emits a single `phonemes` JSON line with no PCM window.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

const LOG_ENV: &str = "PIPER_TTS_LOG";

// ---------------------------------------------------------------------------
// Harness (identical to the one in tests/request_contract.rs)
// ---------------------------------------------------------------------------

struct SidecarHarness {
    child: Child,
    stdout: BufReader<ChildStdout>,
    #[allow(dead_code)]
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

    fn read_json_line(&mut self) -> Value {
        let mut line = String::new();
        let bytes = self
            .stdout
            .read_line(&mut line)
            .expect("stdout should be readable");
        assert!(bytes > 0, "expected a JSON line from the sidecar");
        serde_json::from_str(line.trim_end()).expect("sidecar should emit valid JSON")
    }

    /// Read the raw text of the next newline-terminated line without
    /// deserializing it.  Used to do a byte-exact string comparison on the
    /// pong line.
    fn read_raw_line(&mut self) -> String {
        let mut line = String::new();
        let bytes = self
            .stdout
            .read_line(&mut line)
            .expect("stdout should be readable");
        assert!(bytes > 0, "expected a line from the sidecar");
        line.trim_end_matches(['\n', '\r']).to_string()
    }

    fn close_stdin(&mut self) {
        let _ = self.child.stdin.take();
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

// ---------------------------------------------------------------------------
// Test 1 — ping immediately after ready
// ---------------------------------------------------------------------------

/// Send a single ping right after the `ready` line and assert the pong is
/// byte-exact `{"op":"pong","id":"h1"}`.
#[test]
fn ping_after_ready_returns_pong() {
    let mut sidecar = SidecarHarness::spawn();

    // Must consume the ready line before sending any request.
    let ready = sidecar.read_json_line();
    assert_eq!(ready["op"], "ready", "first line must be the ready line");

    // Send the ping.
    sidecar.send_json(json!({"op": "ping", "id": "h1"}));

    // Read the raw line so we can assert byte-exact equality.
    let pong_line = sidecar.read_raw_line();
    assert_eq!(
        pong_line,
        r#"{"op":"pong","id":"h1"}"#,
        "pong must be byte-exact"
    );
}

// ---------------------------------------------------------------------------
// Test 2 — ping between two phonemize requests
//
// Phonemize emits a single `phonemes` JSON line and NO PCM bytes, so it
// sidesteps the ADR §5.1 ordering invariant (no `audio`/`done` window) and
// the PIPER_TTS_REAL_VOICE_DIR env-gate used in the existing harness.
//
// Interleave order (strictly enforced):
//   1. Write phonemize request A ("hello").
//   2. Consume phonemize response A completely (one `phonemes` JSON line).
//   3. Write the ping.
//   4. Read the pong — must be byte-exact.
//   5. Write phonemize request B ("world").
//   6. Consume phonemize response B completely.
// ---------------------------------------------------------------------------

/// A ping placed strictly between two phonemize responses must return a clean
/// byte-exact pong and must not disturb framing of either phonemize response.
#[test]
fn ping_between_requests_preserves_framing() {
    let mut sidecar = SidecarHarness::spawn();

    // Consume the ready line first.
    let ready = sidecar.read_json_line();
    assert_eq!(ready["op"], "ready", "first line must be the ready line");

    // --- Request A (phonemize "hello") ---
    sidecar.send_json(json!({
        "op": "phonemize",
        "id": "phon-a",
        "text": "hello",
        "language": "en-US",
    }));

    // Consume response A fully before touching the ping.
    let resp_a = sidecar.read_json_line();
    assert_eq!(resp_a["op"], "phonemes", "response A must be phonemes");
    assert_eq!(resp_a["id"], "phon-a", "response A id must match");
    assert!(
        !resp_a["phonemes"].as_str().unwrap_or("").is_empty(),
        "phonemes for 'hello' must not be empty"
    );

    // --- Ping between the two requests (ADR §5.1 invariant satisfied: no
    //     `audio` line has been emitted, so there is no PCM window to violate) ---
    sidecar.send_json(json!({"op": "ping", "id": "h1"}));

    // Read the pong with byte-exact assertion.
    let pong_line = sidecar.read_raw_line();
    assert_eq!(
        pong_line,
        r#"{"op":"pong","id":"h1"}"#,
        "pong between phonemize requests must be byte-exact"
    );

    // --- Request B (phonemize "world") — must succeed normally after ping ---
    sidecar.send_json(json!({
        "op": "phonemize",
        "id": "phon-b",
        "text": "world",
        "language": "en-US",
    }));

    let resp_b = sidecar.read_json_line();
    assert_eq!(resp_b["op"], "phonemes", "response B must be phonemes");
    assert_eq!(resp_b["id"], "phon-b", "response B id must match");
    assert!(
        !resp_b["phonemes"].as_str().unwrap_or("").is_empty(),
        "phonemes for 'world' must not be empty"
    );
}
