use std::collections::HashMap;

use ndarray::{Array1, Array2};
use ort::session::Session;
use ort::value::Tensor;
use serde::Deserialize;

use crate::PiperError;
use crate::PiperResult;

pub const BOS: char = '^';
pub const EOS: char = '$';
pub const PAD: char = '_';

#[derive(Deserialize)]
pub struct AudioConfig {
    pub sample_rate: u32,
}

#[derive(Deserialize)]
pub struct ESpeakConfig {
    pub voice: String,
}

#[derive(Deserialize, Clone)]
pub struct InferenceConfig {
    pub noise_scale: f32,
    pub length_scale: f32,
    pub noise_w: f32,
}

#[derive(Deserialize)]
pub struct ModelConfig {
    pub audio: AudioConfig,
    pub espeak: ESpeakConfig,
    pub inference: InferenceConfig,
    pub num_speakers: u32,
    #[serde(default, deserialize_with = "deserialize_lenient_speaker_id_map")]
    pub speaker_id_map: HashMap<String, i64>,
    pub phoneme_id_map: HashMap<char, Vec<i64>>,
    // Preserved-but-not-consumed: accepted leniently so non-canonical shapes
    // that the host previously normalized never panic this deserializer.
    #[serde(default, deserialize_with = "deserialize_lenient_phoneme_map")]
    pub phoneme_map: HashMap<i64, Option<String>>,
}

fn deserialize_lenient_speaker_id_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde_json::Value;
    let value = Value::deserialize(deserializer)?;
    let Value::Object(obj) = value else {
        tracing::debug!(
            shape = ?value,
            "speaker_id_map not an object; falling back to empty map"
        );
        return Ok(HashMap::new());
    };
    let mut out = HashMap::with_capacity(obj.len());
    for (k, v) in obj {
        let Some(i) = v.as_i64() else {
            tracing::debug!(
                key = %k, value = ?v,
                "speaker_id_map value not integer; falling back to empty map"
            );
            return Ok(HashMap::new());
        };
        out.insert(k, i);
    }
    Ok(out)
}

fn deserialize_lenient_phoneme_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<i64, Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde_json::Value;
    let value = Value::deserialize(deserializer)?;
    let Value::Object(obj) = value else {
        tracing::debug!(shape = ?value, "phoneme_map not an object; falling back to empty map");
        return Ok(HashMap::new());
    };
    let mut out = HashMap::with_capacity(obj.len());
    for (k, v) in obj {
        let Ok(key) = k.parse::<i64>() else {
            tracing::debug!(key = %k, "phoneme_map key not int-parseable; falling back to empty map");
            return Ok(HashMap::new());
        };
        let val = match v {
            Value::String(s) => Some(s),
            Value::Null => None,
            other => {
                tracing::debug!(key = %k, value = ?other, "phoneme_map value not string|null; falling back to empty map");
                return Ok(HashMap::new());
            }
        };
        out.insert(key, val);
    }
    Ok(out)
}

pub fn phonemes_to_ids(config: &ModelConfig, phonemes: &str) -> Vec<i64> {
    let map = &config.phoneme_id_map;
    let pad_id = *map.get(&PAD).and_then(|v| v.first()).unwrap_or(&0);
    let bos_id = *map.get(&BOS).and_then(|v| v.first()).unwrap_or(&0);
    let eos_id = *map.get(&EOS).and_then(|v| v.first()).unwrap_or(&0);

    let mut ids = Vec::with_capacity((phonemes.len() + 1) * 2);
    ids.push(bos_id);
    for ch in phonemes.chars() {
        if let Some(id) = map.get(&ch).and_then(|v| v.first()) {
            ids.push(*id);
            ids.push(pad_id);
        }
    }
    ids.push(eos_id);
    ids
}

pub fn infer(
    session: &mut Session,
    config: &ModelConfig,
    phonemes: &str,
    noise_scale: f32,
    length_scale: f32,
    noise_w: f32,
    speaker_id: i64,
) -> PiperResult<Vec<f32>> {
    let ids = phonemes_to_ids(config, phonemes);
    let input_len = ids.len();
    let input = Array2::<i64>::from_shape_vec((1, input_len), ids).unwrap();
    let input_lengths = Array1::<i64>::from_iter([input_len as i64]);
    let scales = Array1::<f32>::from_iter([noise_scale, length_scale, noise_w]);

    let input_t = Tensor::<i64>::from_array(([1, input_len], input.into_raw_vec_and_offset().0.into_boxed_slice())).unwrap();
    let lengths_t = Tensor::<i64>::from_array(([1], input_lengths.into_raw_vec_and_offset().0.into_boxed_slice())).unwrap();
    let scales_t = Tensor::<f32>::from_array(([3], scales.into_raw_vec_and_offset().0.into_boxed_slice())).unwrap();

    let outputs = if config.num_speakers > 1 {
        let sid = Array1::<i64>::from_iter([speaker_id]);
        let sid_t = Tensor::<i64>::from_array(([1], sid.into_raw_vec_and_offset().0.into_boxed_slice())).unwrap();
        session.run(ort::inputs![input_t, lengths_t, scales_t, sid_t])
    } else {
        session.run(ort::inputs![input_t, lengths_t, scales_t])
    }.map_err(|e| PiperError::InferenceError(format!("Inference failed: {}", e)))?;

    let (_, audio) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| PiperError::InferenceError(format!("Failed to extract output: {}", e)))?;

    Ok(audio.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_json(speaker_id_map: &str, phoneme_map_field: &str) -> String {
        format!(
            r#"{{
                "audio": {{ "sample_rate": 22050 }},
                "espeak": {{ "voice": "en-us" }},
                "inference": {{ "noise_scale": 0.667, "length_scale": 1.0, "noise_w": 0.8 }},
                "num_speakers": 1,
                "speaker_id_map": {speaker_id_map},
                "phoneme_id_map": {{ "a": [1] }}{phoneme_map_field}
            }}"#
        )
    }

    fn parse(speaker_id_map: &str, phoneme_map_field: &str) -> ModelConfig {
        serde_json::from_str(&config_json(speaker_id_map, phoneme_map_field))
            .expect("lenient deserializer must accept host-normalized shapes")
    }

    #[test]
    fn speaker_id_map_canonical_preserved() {
        let cfg = parse(r#"{"alice": 0, "bob": 1}"#, "");
        assert_eq!(cfg.speaker_id_map.get("alice"), Some(&0));
        assert_eq!(cfg.speaker_id_map.get("bob"), Some(&1));
    }

    #[test]
    fn speaker_id_map_null_falls_back_to_empty() {
        let cfg = parse("null", "");
        assert!(cfg.speaker_id_map.is_empty());
    }

    #[test]
    fn speaker_id_map_array_falls_back_to_empty() {
        let cfg = parse("[0, 1, 2]", "");
        assert!(cfg.speaker_id_map.is_empty());
    }

    #[test]
    fn speaker_id_map_non_integer_value_falls_back_to_empty() {
        let cfg = parse(r#"{"alice": "zero"}"#, "");
        assert!(cfg.speaker_id_map.is_empty());
    }

    #[test]
    fn speaker_id_map_missing_defaults_empty() {
        // omit speaker_id_map entirely
        let json = r#"{
            "audio": { "sample_rate": 22050 },
            "espeak": { "voice": "en-us" },
            "inference": { "noise_scale": 0.667, "length_scale": 1.0, "noise_w": 0.8 },
            "num_speakers": 1,
            "phoneme_id_map": { "a": [1] }
        }"#;
        let cfg: ModelConfig = serde_json::from_str(json).expect("missing field must default");
        assert!(cfg.speaker_id_map.is_empty());
        assert!(cfg.phoneme_map.is_empty());
    }

    #[test]
    fn phoneme_map_canonical_preserved() {
        let cfg = parse(r#"{}"#, r#", "phoneme_map": {"97": "a", "98": null}"#);
        assert_eq!(cfg.phoneme_map.get(&97), Some(&Some("a".to_string())));
        assert_eq!(cfg.phoneme_map.get(&98), Some(&None));
    }

    #[test]
    fn phoneme_map_non_object_falls_back_to_empty() {
        let cfg = parse(r#"{}"#, r#", "phoneme_map": [1, 2, 3]"#);
        assert!(cfg.phoneme_map.is_empty());
    }

    #[test]
    fn phoneme_map_non_int_key_falls_back_to_empty() {
        let cfg = parse(r#"{}"#, r#", "phoneme_map": {"a": "x"}"#);
        assert!(cfg.phoneme_map.is_empty());
    }

    #[test]
    fn phoneme_map_bad_value_type_falls_back_to_empty() {
        let cfg = parse(r#"{}"#, r#", "phoneme_map": {"97": 42}"#);
        assert!(cfg.phoneme_map.is_empty());
    }
}
