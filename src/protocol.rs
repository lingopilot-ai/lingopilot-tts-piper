use serde::{Deserialize, Serialize};

const MAX_TEXT_CHARS: usize = 8192;
const MIN_SPEED: f32 = 0.5;
const MAX_SPEED: f32 = 5.5;

/// Request sent by the host process via stdin (one JSON object per line).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TtsRequest {
    /// Text to synthesize.
    pub text: String,

    /// Piper voice ID (e.g. "en_US-hfc_female-medium").
    pub voice: String,

    /// Playback speed multiplier (1.0 = normal).
    #[serde(default = "default_speed")]
    pub speed: f32,

    /// Piper model directory path (absolute).
    pub model_dir: String,
}

fn default_speed() -> f32 {
    1.0
}

impl TtsRequest {
    /// Validate request semantics after JSON deserialization succeeds.
    pub fn validate(&self) -> Result<(), String> {
        if self.text.trim().is_empty() {
            return Err("Invalid text: text must not be empty or whitespace".to_string());
        }

        if self.text.chars().count() > MAX_TEXT_CHARS {
            return Err(format!(
                "Invalid text: text must be at most {MAX_TEXT_CHARS} characters"
            ));
        }

        if !self.speed.is_finite() || !(MIN_SPEED..=MAX_SPEED).contains(&self.speed) {
            return Err(format!(
                "Invalid speed: speed must be a finite number between {MIN_SPEED} and {MAX_SPEED}"
            ));
        }

        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::TtsRequest;

    fn parse_request(text: &str, speed: f32) -> TtsRequest {
        serde_json::from_str(&format!(
            r#"{{
                "text":{text:?},
                "voice":"en_US-hfc_female-medium",
                "speed":{speed},
                "model_dir":"C:\\voices\\en_US-hfc_female-medium"
            }}"#
        ))
        .expect("request should deserialize")
    }

    #[test]
    fn request_deserializes_without_legacy_espeak_field() {
        let request = r#"{
            "text":"Hello",
            "voice":"en_US-hfc_female-medium",
            "speed":1.0,
            "model_dir":"C:\\voices\\en_US-hfc_female-medium"
        }"#;

        let parsed: TtsRequest = serde_json::from_str(request).expect("request should deserialize");

        assert_eq!(parsed.text, "Hello");
        assert_eq!(parsed.voice, "en_US-hfc_female-medium");
        assert_eq!(parsed.speed, 1.0);
    }

    #[test]
    fn request_rejects_legacy_espeak_data_dir_as_unknown_field() {
        let request = r#"{
            "text":"Hello",
            "voice":"en_US-hfc_female-medium",
            "speed":1.0,
            "model_dir":"C:\\voices\\en_US-hfc_female-medium",
            "espeak_data_dir":"C:\\runtime\\espeak-runtime"
        }"#;

        let error = serde_json::from_str::<TtsRequest>(request).expect_err("request should fail");
        let message = error.to_string();

        assert!(message.contains("unknown field `espeak_data_dir`"));
    }

    #[test]
    fn request_rejects_language_as_unknown_field() {
        let request = r#"{
            "text":"Hello",
            "language":"en",
            "voice":"en_US-hfc_female-medium",
            "speed":1.0,
            "model_dir":"C:\\voices\\en_US-hfc_female-medium"
        }"#;

        let error = serde_json::from_str::<TtsRequest>(request).expect_err("request should fail");
        let message = error.to_string();

        assert!(message.contains("unknown field `language`"));
    }

    #[test]
    fn request_validation_rejects_empty_text() {
        let error = parse_request("", 1.0)
            .validate()
            .expect_err("empty text should fail");

        assert!(error.contains("text must not be empty or whitespace"));
    }

    #[test]
    fn request_validation_rejects_whitespace_only_text() {
        let error = parse_request("   \n\t  ", 1.0)
            .validate()
            .expect_err("whitespace-only text should fail");

        assert!(error.contains("text must not be empty or whitespace"));
    }

    #[test]
    fn request_validation_accepts_text_at_max_length() {
        let text = "a".repeat(8192);

        parse_request(&text, 1.0)
            .validate()
            .expect("text at limit should pass");
    }

    #[test]
    fn request_validation_rejects_text_above_max_length() {
        let text = "a".repeat(8193);
        let error = parse_request(&text, 1.0)
            .validate()
            .expect_err("text above limit should fail");

        assert!(error.contains("text must be at most 8192 characters"));
    }

    #[test]
    fn request_validation_accepts_speed_at_lower_bound() {
        parse_request("Hello", 0.5)
            .validate()
            .expect("lower speed bound should pass");
    }

    #[test]
    fn request_validation_accepts_speed_at_upper_bound() {
        parse_request("Hello", 5.5)
            .validate()
            .expect("upper speed bound should pass");
    }

    #[test]
    fn request_validation_rejects_speed_below_lower_bound() {
        let error = parse_request("Hello", 0.49)
            .validate()
            .expect_err("speed below range should fail");

        assert!(error.contains("speed must be a finite number between 0.5 and 5.5"));
    }

    #[test]
    fn request_validation_rejects_speed_above_upper_bound() {
        let error = parse_request("Hello", 5.51)
            .validate()
            .expect_err("speed above range should fail");

        assert!(error.contains("speed must be a finite number between 0.5 and 5.5"));
    }
}
