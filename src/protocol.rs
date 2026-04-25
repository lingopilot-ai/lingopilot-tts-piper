use serde::{Deserialize, Serialize};

pub const SAMPLE_RATE_HZ: u32 = 22050;
pub const CHANNELS: u16 = 1;
pub const ENCODING: &str = "pcm16le";
pub const SUPPORTED_OPS: [&str; 2] = ["synthesize", "phonemize"];

const MAX_TEXT_CHARS: usize = 8192;
const MAX_ID_BYTES: usize = 128;
const MIN_SPEED: f32 = 0.5;
const MAX_SPEED: f32 = 2.0;

/// Request received on stdin, discriminated by the `op` field.
#[derive(Debug, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum SidecarRequest {
    #[serde(rename = "synthesize")]
    Synthesize(SynthesizeRequest),
    #[serde(rename = "phonemize")]
    Phonemize(PhonemizeRequest),
    #[serde(rename = "ping")]
    Ping(PingRequest),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PingRequest {
    pub id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SynthesizeRequest {
    pub id: String,
    pub text: String,
    pub voice_model_path: String,
    pub voice_config_path: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub speaker_id: i64,
    #[serde(default = "default_speed")]
    pub speed: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhonemizeRequest {
    pub id: String,
    pub text: String,
    pub language: String,
}

fn default_speed() -> f32 {
    1.0
}

impl SidecarRequest {
    pub fn id(&self) -> &str {
        match self {
            SidecarRequest::Synthesize(r) => &r.id,
            SidecarRequest::Phonemize(r) => &r.id,
            SidecarRequest::Ping(r) => &r.id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            SidecarRequest::Synthesize(r) => r.validate(),
            SidecarRequest::Phonemize(r) => r.validate(),
            SidecarRequest::Ping(r) => validate_id(&r.id),
        }
    }
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("id must not be empty".to_string());
    }
    if id.len() > MAX_ID_BYTES {
        return Err(format!("id must be at most {MAX_ID_BYTES} bytes"));
    }
    Ok(())
}

fn validate_text(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("text must not be empty or whitespace".to_string());
    }
    validate_text_length(text)
}

fn validate_text_length(text: &str) -> Result<(), String> {
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!("text must be at most {MAX_TEXT_CHARS} characters"));
    }
    Ok(())
}

impl SynthesizeRequest {
    pub fn validate(&self) -> Result<(), String> {
        validate_id(&self.id)?;
        validate_text(&self.text)?;
        if self.voice_model_path.trim().is_empty() {
            return Err("voice_model_path must not be empty or whitespace".to_string());
        }
        if self.voice_config_path.trim().is_empty() {
            return Err("voice_config_path must not be empty or whitespace".to_string());
        }
        if !self.speed.is_finite() {
            return Err("speed must be a finite number".to_string());
        }
        Ok(())
    }

    /// Speed clamped to the directive range [0.5, 2.0].
    pub fn clamped_speed(&self) -> f32 {
        self.speed.clamp(MIN_SPEED, MAX_SPEED)
    }
}

impl PhonemizeRequest {
    pub fn validate(&self) -> Result<(), String> {
        validate_id(&self.id)?;
        // Empty / whitespace / punct-only text is legal for phonemize per
        // directive 2026-04-22e §P1.4; the handler returns {phonemes:"", words:[]}.
        validate_text_length(&self.text)?;
        if self.language.trim().is_empty() {
            return Err("language must not be empty or whitespace".to_string());
        }
        Ok(())
    }
}

/// Per-word entry in a `phonemes` response. `words[].text` reconstructs the
/// request input modulo whitespace; `words[].phonemes` is a best-effort split
/// of the top-level IPA string and is NOT asserted byte-equal to the join of
/// the top-level per directive 2026-04-22e.
#[derive(Debug, Serialize)]
pub struct WordEntry<'a> {
    pub text: &'a str,
    pub phonemes: &'a str,
}

/// Response emitted on stdout, discriminated by the `op` field.
#[derive(Debug, Serialize)]
#[serde(tag = "op")]
pub enum SidecarResponse<'a> {
    #[serde(rename = "ready")]
    Ready {
        version: &'a str,
        sample_rate: u32,
        channels: u16,
        encoding: &'a str,
        ops: &'a [&'a str],
    },
    #[serde(rename = "audio")]
    Audio {
        id: &'a str,
        bytes: u32,
        sample_rate: u32,
        channels: u16,
    },
    #[serde(rename = "done")]
    Done { id: &'a str },
    #[serde(rename = "phonemes")]
    Phonemes {
        id: &'a str,
        phonemes: &'a str,
        words: &'a [WordEntry<'a>],
    },
    #[serde(rename = "error")]
    Error {
        id: Option<&'a str>,
        kind: &'a str,
        message: &'a str,
    },
    #[serde(rename = "pong")]
    Pong { id: &'a str },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_response_matches_directive_line() {
        let ops: &[&str] = &SUPPORTED_OPS;
        let response = SidecarResponse::Ready {
            version: "0.1.6",
            sample_rate: SAMPLE_RATE_HZ,
            channels: CHANNELS,
            encoding: ENCODING,
            ops,
        };
        let json = serde_json::to_string(&response).expect("should serialize");
        assert_eq!(
            json,
            r#"{"op":"ready","version":"0.1.6","sample_rate":22050,"channels":1,"encoding":"pcm16le","ops":["synthesize","phonemize"]}"#
        );
    }

    #[test]
    fn phonemes_response_serializes_with_empty_words_array() {
        let response = SidecarResponse::Phonemes {
            id: "p1",
            phonemes: "",
            words: &[],
        };
        let json = serde_json::to_string(&response).expect("should serialize");
        assert_eq!(json, r#"{"op":"phonemes","id":"p1","phonemes":"","words":[]}"#);
    }

    #[test]
    fn phonemes_response_serializes_word_entries() {
        let words = [
            WordEntry { text: "hello", phonemes: "həlˈoʊ" },
            WordEntry { text: "world", phonemes: "wˈɜːld" },
        ];
        let response = SidecarResponse::Phonemes {
            id: "p1",
            phonemes: "həlˈoʊ wˈɜːld",
            words: &words,
        };
        let json = serde_json::to_string(&response).expect("should serialize");
        assert!(json.contains(r#""words":[{"text":"hello","phonemes":"həlˈoʊ"}"#));
        assert!(json.contains(r#"{"text":"world","phonemes":"wˈɜːld"}"#));
    }

    #[test]
    fn synthesize_request_deserializes_with_defaults() {
        let request = r#"{"op":"synthesize","id":"r1","text":"Hi","voice_model_path":"C:/v/x.onnx","voice_config_path":"C:/v/x.onnx.json"}"#;
        let parsed: SidecarRequest =
            serde_json::from_str(request).expect("synthesize should parse");
        match parsed {
            SidecarRequest::Synthesize(r) => {
                assert_eq!(r.id, "r1");
                assert_eq!(r.speed, 1.0);
                assert_eq!(r.speaker_id, 0);
            }
            _ => panic!("expected synthesize"),
        }
    }

    #[test]
    fn phonemize_request_deserializes() {
        let request =
            r#"{"op":"phonemize","id":"r2","text":"hello","language":"en"}"#;
        let parsed: SidecarRequest =
            serde_json::from_str(request).expect("phonemize should parse");
        match parsed {
            SidecarRequest::Phonemize(r) => {
                assert_eq!(r.id, "r2");
                assert_eq!(r.language, "en");
            }
            _ => panic!("expected phonemize"),
        }
    }

    #[test]
    fn unknown_op_is_rejected() {
        let request =
            r#"{"op":"cancel","id":"r3"}"#;
        let error = serde_json::from_str::<SidecarRequest>(request)
            .expect_err("unknown op should fail");
        assert!(error.to_string().contains("unknown variant"));
    }

    #[test]
    fn legacy_type_field_is_rejected() {
        let request = r#"{"type":"synthesize","id":"r1","text":"hi","voice_model_path":"a","voice_config_path":"b"}"#;
        let error =
            serde_json::from_str::<SidecarRequest>(request).expect_err("type is not a valid tag");
        let msg = error.to_string();
        assert!(msg.contains("missing field `op`") || msg.contains("unknown field `type`"));
    }

    #[test]
    fn synthesize_rejects_empty_id() {
        let r = SynthesizeRequest {
            id: String::new(),
            text: "hi".to_string(),
            voice_model_path: "a".to_string(),
            voice_config_path: "b".to_string(),
            speaker_id: 0,
            speed: 1.0,
        };
        assert!(r.validate().is_err());
    }

    #[test]
    fn synthesize_rejects_id_longer_than_128_bytes() {
        let r = SynthesizeRequest {
            id: "x".repeat(129),
            text: "hi".to_string(),
            voice_model_path: "a".to_string(),
            voice_config_path: "b".to_string(),
            speaker_id: 0,
            speed: 1.0,
        };
        let e = r.validate().expect_err("long id should fail");
        assert!(e.contains("128 bytes"));
    }

    #[test]
    fn synthesize_rejects_whitespace_text() {
        let r = SynthesizeRequest {
            id: "r1".to_string(),
            text: "   ".to_string(),
            voice_model_path: "a".to_string(),
            voice_config_path: "b".to_string(),
            speaker_id: 0,
            speed: 1.0,
        };
        assert!(r.validate().is_err());
    }

    #[test]
    fn speed_is_clamped_to_directive_range() {
        let low = SynthesizeRequest {
            id: "r1".to_string(),
            text: "hi".to_string(),
            voice_model_path: "a".to_string(),
            voice_config_path: "b".to_string(),
            speaker_id: 0,
            speed: 0.1,
        };
        assert_eq!(low.clamped_speed(), 0.5);

        let high = SynthesizeRequest {
            id: "r1".to_string(),
            text: "hi".to_string(),
            voice_model_path: "a".to_string(),
            voice_config_path: "b".to_string(),
            speaker_id: 0,
            speed: 3.0,
        };
        assert_eq!(high.clamped_speed(), 2.0);
    }

    #[test]
    fn phonemize_rejects_empty_language() {
        let r = PhonemizeRequest {
            id: "r1".to_string(),
            text: "hi".to_string(),
            language: "".to_string(),
        };
        assert!(r.validate().is_err());
    }

    #[test]
    fn phonemize_accepts_empty_or_whitespace_text() {
        // Directive 2026-04-22e §P1.4: empty text is legal for phonemize.
        for text in ["", "   ", "\t\n"] {
            let r = PhonemizeRequest {
                id: "r1".to_string(),
                text: text.to_string(),
                language: "en-US".to_string(),
            };
            assert!(
                r.validate().is_ok(),
                "phonemize should accept text {text:?} after directive 2026-04-22e"
            );
        }
    }

    #[test]
    fn phonemize_still_rejects_text_above_char_limit() {
        let r = PhonemizeRequest {
            id: "r1".to_string(),
            text: "a".repeat(MAX_TEXT_CHARS + 1),
            language: "en-US".to_string(),
        };
        let err = r.validate().expect_err("oversized text should fail");
        assert!(err.contains("8192"));
    }

    #[test]
    fn reserved_ops_are_unknown_variants() {
        for op in ["cancel", "audio_chunk"] {
            let request = format!(r#"{{"op":"{op}","id":"r1"}}"#);
            let error = serde_json::from_str::<SidecarRequest>(&request)
                .expect_err("reserved op should not deserialize");
            assert!(error.to_string().contains("unknown variant"));
        }
    }

    // --- H-01 ping / pong tests (ADR §4.2) ---

    #[test]
    fn ping_request_deserializes() {
        let request = r#"{"op":"ping","id":"h1"}"#;
        let parsed: SidecarRequest =
            serde_json::from_str(request).expect("ping should deserialize");
        match parsed {
            SidecarRequest::Ping(r) => assert_eq!(r.id, "h1"),
            _ => panic!("expected Ping variant"),
        }
    }

    #[test]
    fn ping_request_rejects_extra_fields() {
        let request = r#"{"op":"ping","id":"h1","extra":"bad"}"#;
        let error = serde_json::from_str::<SidecarRequest>(request)
            .expect_err("extra field should be rejected by deny_unknown_fields");
        assert!(
            error.to_string().contains("unknown field") || error.to_string().contains("extra"),
            "unexpected error message: {error}"
        );
    }

    #[test]
    fn ping_request_rejects_empty_id() {
        let req = SidecarRequest::Ping(PingRequest { id: String::new() });
        let err = req.validate().expect_err("empty id should fail");
        assert!(err.contains("empty"), "unexpected error: {err}");
    }

    #[test]
    fn ping_request_rejects_oversize_id() {
        let req = SidecarRequest::Ping(PingRequest { id: "x".repeat(129) });
        let err = req.validate().expect_err("129-byte id should fail");
        assert!(err.contains("128 bytes"), "unexpected error: {err}");
    }

    #[test]
    fn pong_response_serializes() {
        let response = SidecarResponse::Pong { id: "h1" };
        let json = serde_json::to_string(&response).expect("pong should serialize");
        assert_eq!(json, r#"{"op":"pong","id":"h1"}"#);
    }
}
