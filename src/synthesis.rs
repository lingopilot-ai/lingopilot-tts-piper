use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use piper_rs::from_config_path;
use piper_rs::synth::{AudioOutputConfig, PiperSpeechSynthesizer};

type SynthHandle = Arc<PiperSpeechSynthesizer>;
type SynthLoader = dyn Fn(&Path) -> Result<SynthHandle, String> + Send + Sync;

/// Result of a successful TTS synthesis.
pub struct SynthResult {
    /// Raw PCM16 LE samples (mono).
    pub pcm16: Vec<i16>,
    /// Sample rate reported by the Piper model. The sidecar always re-emits
    /// 22050 Hz per the directive contract, so this value is informational.
    #[allow(dead_code)]
    pub sample_rate: u32,
}

/// Process-owned Piper model cache keyed by the resolved voice config path.
pub struct SynthesisCache {
    models: HashMap<PathBuf, SynthHandle>,
    loader: Box<SynthLoader>,
}

impl SynthesisCache {
    /// Create a cache that loads Piper models on first use and reuses them
    /// for the lifetime of the process.
    pub fn new() -> Self {
        Self::with_loader(load_model_from_config)
    }

    fn with_loader<L>(loader: L) -> Self
    where
        L: Fn(&Path) -> Result<SynthHandle, String> + Send + Sync + 'static,
    {
        Self {
            models: HashMap::new(),
            loader: Box::new(loader),
        }
    }

    /// Synthesize text using a cached Piper model for the resolved voice.
    pub fn synthesize(
        &mut self,
        text: &str,
        voice_config_path: &Path,
        speed: f32,
    ) -> Result<SynthResult, String> {
        let synth = self.get_or_load_synth(voice_config_path)?;
        synthesize_with_synth(synth.as_ref(), text, speed)
    }

    fn get_or_load_synth(&mut self, voice_config_path: &Path) -> Result<SynthHandle, String> {
        get_or_load_cached(
            &mut self.models,
            voice_config_path,
            &self.loader,
            "voice config",
        )
    }
}

fn load_model_from_config(config_path: &Path) -> Result<SynthHandle, String> {
    let model =
        from_config_path(config_path).map_err(|e| format!("Failed to load Piper voice: {}", e))?;
    let synth = PiperSpeechSynthesizer::new(model)
        .map_err(|e| format!("Failed to create synthesizer: {}", e))?;
    Ok(Arc::new(synth))
}

fn get_or_load_cached<T, L>(
    cache: &mut HashMap<PathBuf, Arc<T>>,
    cache_key: &Path,
    loader: &L,
    cache_label: &str,
) -> Result<Arc<T>, String>
where
    T: Send + Sync + 'static,
    L: Fn(&Path) -> Result<Arc<T>, String> + ?Sized,
{
    if let Some(value) = cache.get(cache_key) {
        tracing::debug!(
            event = "synth_cache_hit",
            cache_label,
            cache_key = cache_key.display().to_string()
        );
        return Ok(Arc::clone(value));
    }

    tracing::debug!(
        event = "synth_cache_miss",
        cache_label,
        cache_key = cache_key.display().to_string()
    );

    let value = match loader(cache_key) {
        Ok(value) => value,
        Err(error) => {
            tracing::debug!(
                event = "synth_cache_load_failed",
                cache_label,
                cache_key = cache_key.display().to_string(),
                detail = error.as_str()
            );
            return Err(error);
        }
    };

    cache.insert(cache_key.to_path_buf(), Arc::clone(&value));
    Ok(value)
}

/// Validate the process-scoped eSpeak runtime directory.
pub fn validate_espeak_data_dir(data_dir: &Path) -> Result<(), String> {
    if !data_dir.is_absolute() {
        return Err(format!(
            "Invalid eSpeak data dir '{}': path must be absolute",
            data_dir.display()
        ));
    }

    let metadata = std::fs::metadata(data_dir).map_err(|error| {
        format!(
            "Cannot use eSpeak data dir '{}': {}",
            data_dir.display(),
            error
        )
    })?;
    if !metadata.is_dir() {
        return Err(format!(
            "Cannot use eSpeak data dir '{}': path is not a directory",
            data_dir.display()
        ));
    }

    let espeak_ng_data_dir = data_dir.join("espeak-ng-data");
    let metadata = std::fs::metadata(&espeak_ng_data_dir).map_err(|error| {
        format!(
            "Invalid eSpeak data dir '{}': missing 'espeak-ng-data' directory ({})",
            data_dir.display(),
            error
        )
    })?;
    if !metadata.is_dir() {
        return Err(format!(
            "Invalid eSpeak data dir '{}': '{}' is not a directory",
            data_dir.display(),
            espeak_ng_data_dir.display()
        ));
    }

    Ok(())
}

/// Validate a request-provided absolute file path (Piper voice model or config).
pub fn validate_voice_file(path: &Path, kind: &str) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!(
            "Invalid {kind} '{}': path must be absolute",
            path.display()
        ));
    }

    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(format!(
            "Invalid {kind} '{}': path is not a file",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(format!(
            "Invalid {kind} '{}': path does not exist",
            path.display()
        )),
        Err(error) => Err(format!(
            "Cannot use {kind} '{}': {}",
            path.display(),
            error
        )),
    }
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

fn synthesize_with_synth(
    synth: &PiperSpeechSynthesizer,
    text: &str,
    speed: f32,
) -> Result<SynthResult, String> {
    let sample_rate = synth
        .clone_model()
        .audio_output_info()
        .map_err(|e| format!("Failed to get audio info: {}", e))?
        .sample_rate as u32;

    let audio_cfg = AudioOutputConfig {
        rate: speed_to_rate_percent(speed),
        volume: None,
        pitch: None,
        appended_silence_ms: None,
    };

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


#[cfg(test)]
mod tests {
    use super::{
        get_or_load_cached, validate_espeak_data_dir, validate_voice_file,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos();

            let path = std::env::temp_dir().join(format!(
                "lingopilot-tts-piper-{prefix}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("temp dir should be created");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[derive(Debug)]
    struct FakeLoadedVoice(&'static str);

    fn create_espeak_runtime(dir: &Path) {
        fs::create_dir(dir.join("espeak-ng-data")).expect("runtime data dir should be created");
    }

    fn unique_missing_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lingopilot-tts-piper-{prefix}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ))
    }

    fn create_voice_config(dir: &Path, voice_id: &str) -> PathBuf {
        fs::write(dir.join(format!("{voice_id}.onnx")), b"model").expect("model should be created");
        let config_path = dir.join(format!("{voice_id}.onnx.json"));
        fs::write(&config_path, b"{}").expect("config should be created");
        config_path
    }

    #[test]
    fn validates_espeak_runtime_dir_when_data_subdir_exists() {
        let temp_dir = TempDir::new("espeak-valid");
        create_espeak_runtime(temp_dir.path());

        validate_espeak_data_dir(temp_dir.path()).expect("runtime dir should validate");
    }

    #[test]
    fn rejects_missing_espeak_runtime_dir() {
        let missing = std::env::temp_dir().join(format!(
            "lingopilot-tts-piper-espeak-missing-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));

        let error =
            validate_espeak_data_dir(&missing).expect_err("missing runtime dir should fail");

        assert!(error.contains("Cannot use eSpeak data dir"));
    }

    #[test]
    fn rejects_non_directory_espeak_runtime_dir() {
        let temp_dir = TempDir::new("espeak-file");
        let file_path = temp_dir.path().join("runtime.txt");
        fs::write(&file_path, b"not a directory").expect("file should be created");

        let error = validate_espeak_data_dir(&file_path)
            .expect_err("non-directory runtime path should fail");

        assert!(error.contains("path is not a directory"));
    }

    #[test]
    fn rejects_directory_without_espeak_ng_data_subdir() {
        let temp_dir = TempDir::new("espeak-no-data");

        let error = validate_espeak_data_dir(temp_dir.path()).expect_err("runtime dir should fail");

        assert!(error.contains("missing 'espeak-ng-data' directory"));
    }

    #[test]
    fn validate_voice_file_accepts_absolute_existing_file() {
        let temp_dir = TempDir::new("voice-file-valid");
        let config_path = create_voice_config(temp_dir.path(), "voice-a");

        validate_voice_file(&config_path, "voice_config_path")
            .expect("existing file should validate");
    }

    #[test]
    fn validate_voice_file_rejects_relative_path() {
        let error = validate_voice_file(Path::new("relative.onnx"), "voice_model_path")
            .expect_err("relative path should fail");
        assert!(error.contains("path must be absolute"));
    }

    #[test]
    fn validate_voice_file_rejects_missing_path() {
        let missing = unique_missing_path("voice-file-missing").join("voice.onnx");
        let error = validate_voice_file(&missing, "voice_model_path")
            .expect_err("missing file should fail");
        assert!(error.contains("path does not exist"));
    }

    #[test]
    fn validate_voice_file_rejects_directory() {
        let temp_dir = TempDir::new("voice-file-dir");
        let error = validate_voice_file(temp_dir.path(), "voice_model_path")
            .expect_err("directory path should fail");
        assert!(error.contains("path is not a file"));
    }

    #[test]
    fn cache_reuses_loaded_model_for_same_config_path() {
        let temp_dir = TempDir::new("cache-same-path");
        let config_path = create_voice_config(temp_dir.path(), "voice-a");

        let load_count = Arc::new(AtomicUsize::new(0));
        let mut cache = std::collections::HashMap::<PathBuf, Arc<FakeLoadedVoice>>::new();
        let loader = {
            let load_count = Arc::clone(&load_count);
            move |_config_path: &Path| {
                load_count.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(FakeLoadedVoice("voice-a")))
            }
        };

        let first = get_or_load_cached(&mut cache, &config_path, &loader, "voice config")
            .expect("first load should succeed");
        let second = get_or_load_cached(&mut cache, &config_path, &loader, "voice config")
            .expect("second load should succeed");

        assert_eq!(first.0, "voice-a");
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(load_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cache_does_not_store_failed_loads() {
        let temp_dir = TempDir::new("cache-failed-loads");
        let config_path = create_voice_config(temp_dir.path(), "voice-a");

        let load_count = Arc::new(AtomicUsize::new(0));
        let mut cache = std::collections::HashMap::<PathBuf, Arc<FakeLoadedVoice>>::new();
        let loader = {
            let load_count = Arc::clone(&load_count);
            move |_config_path: &Path| {
                load_count.fetch_add(1, Ordering::SeqCst);
                Err::<Arc<FakeLoadedVoice>, String>("synthetic loader failure".to_string())
            }
        };

        let first = get_or_load_cached(&mut cache, &config_path, &loader, "voice config")
            .expect_err("first load should fail");
        let second = get_or_load_cached(&mut cache, &config_path, &loader, "voice config")
            .expect_err("second load should fail");

        assert_eq!(first, "synthetic loader failure");
        assert_eq!(second, "synthetic loader failure");
        assert_eq!(load_count.load(Ordering::SeqCst), 2);
        assert!(cache.is_empty());
    }
}
