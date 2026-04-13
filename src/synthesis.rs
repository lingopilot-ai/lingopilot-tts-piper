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
    /// Sample rate reported by the Piper model.
    pub sample_rate: u32,
}

/// Exact Piper voice files resolved from a request.
#[derive(Debug)]
pub struct ResolvedVoicePaths {
    /// `<voice_id>.onnx` model file.
    pub model_path: PathBuf,
    /// `<voice_id>.onnx.json` config file.
    pub config_path: PathBuf,
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
    let model = from_config_path(config_path)
        .map_err(|e| format!("Failed to load Piper voice: {}", e))?;
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

/// Validate the request-scoped Piper model directory.
pub fn validate_model_dir(model_dir: &Path) -> Result<(), String> {
    if !model_dir.is_absolute() {
        return Err(format!(
            "Invalid model_dir '{}': path must be absolute",
            model_dir.display()
        ));
    }

    let metadata = match std::fs::metadata(model_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!(
                "Cannot use model_dir '{}': path does not exist",
                model_dir.display()
            ));
        }
        Err(error) => {
            return Err(format!(
                "Cannot use model_dir '{}': {}",
                model_dir.display(),
                error
            ));
        }
    };

    if !metadata.is_dir() {
        return Err(format!(
            "Cannot use model_dir '{}': path is not a directory",
            model_dir.display()
        ));
    }

    Ok(())
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

/// Resolve the exact Piper model/config pair for the requested voice.
pub fn resolve_voice_paths(model_dir: &Path, voice_id: &str) -> Result<ResolvedVoicePaths, String> {
    validate_model_dir(model_dir)?;

    let model_path = model_dir.join(format!("{}.onnx", voice_id));
    ensure_is_file(
        &model_path,
        format!(
            "Requested voice '{}' is missing model file '{}'",
            voice_id,
            model_path.display()
        ),
    )?;

    let config_path = model_dir.join(format!("{}.onnx.json", voice_id));
    ensure_is_file(
        &config_path,
        format!(
            "Requested voice '{}' is missing config file '{}'",
            voice_id,
            config_path.display()
        ),
    )?;

    Ok(ResolvedVoicePaths {
        model_path,
        config_path,
    })
}

fn ensure_is_file(path: &Path, missing_message: String) -> Result<(), String> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(format!(
            "Expected file but found non-file path '{}'",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(missing_message),
        Err(error) => Err(format!("Cannot use path '{}': {}", path.display(), error)),
    }
}

#[cfg(test)]
mod tests {
    use super::{get_or_load_cached, resolve_voice_paths, validate_espeak_data_dir, validate_model_dir};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
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

    fn create_voice_pair(dir: &Path, voice_id: &str) {
        fs::write(dir.join(format!("{voice_id}.onnx")), b"model").expect("model should be created");
        fs::write(dir.join(format!("{voice_id}.onnx.json")), b"{}")
            .expect("config should be created");
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
    fn validates_absolute_existing_model_dir() {
        let temp_dir = TempDir::new("model-valid");

        validate_model_dir(temp_dir.path()).expect("model dir should validate");
    }

    #[test]
    fn rejects_relative_model_dir() {
        let error = validate_model_dir(Path::new("relative-model-dir"))
            .expect_err("relative path should fail");

        assert!(error.contains("path must be absolute"));
    }

    #[test]
    fn rejects_missing_model_dir() {
        let missing = unique_missing_path("model-missing");
        let error = validate_model_dir(&missing).expect_err("missing model dir should fail");

        assert!(error.contains("path does not exist"));
    }

    #[test]
    fn rejects_model_dir_when_path_is_a_file() {
        let temp_dir = TempDir::new("model-file");
        let file_path = temp_dir.path().join("voice.onnx");
        fs::write(&file_path, b"model").expect("file should be created");

        let error = validate_model_dir(&file_path).expect_err("file path should fail");

        assert!(error.contains("path is not a directory"));
    }

    #[test]
    fn resolves_exact_voice_pair_when_both_files_exist() {
        let temp_dir = TempDir::new("resolve-one");
        create_voice_pair(temp_dir.path(), "voice-a");

        let resolved =
            resolve_voice_paths(temp_dir.path(), "voice-a").expect("voice should resolve");

        assert_eq!(resolved.model_path, temp_dir.path().join("voice-a.onnx"));
        assert_eq!(
            resolved.config_path,
            temp_dir.path().join("voice-a.onnx.json")
        );
    }

    #[test]
    fn resolves_requested_voice_when_multiple_pairs_exist() {
        let temp_dir = TempDir::new("resolve-many");
        create_voice_pair(temp_dir.path(), "voice-a");
        create_voice_pair(temp_dir.path(), "voice-b");

        let resolved =
            resolve_voice_paths(temp_dir.path(), "voice-b").expect("voice should resolve");

        assert_eq!(resolved.model_path, temp_dir.path().join("voice-b.onnx"));
        assert_eq!(
            resolved.config_path,
            temp_dir.path().join("voice-b.onnx.json")
        );
    }

    #[test]
    fn rejects_missing_requested_voice_even_when_other_pairs_exist() {
        let temp_dir = TempDir::new("resolve-missing");
        create_voice_pair(temp_dir.path(), "voice-a");
        create_voice_pair(temp_dir.path(), "voice-b");

        let error = resolve_voice_paths(temp_dir.path(), "voice-c").expect_err("voice should fail");

        assert!(error.contains("voice-c"));
        assert!(error.contains("voice-c.onnx"));
    }

    #[test]
    fn rejects_directory_without_any_matching_config() {
        let temp_dir = TempDir::new("resolve-no-config");
        fs::write(temp_dir.path().join("voice-a.onnx"), b"model").expect("model should be created");

        let error = resolve_voice_paths(temp_dir.path(), "voice-a").expect_err("voice should fail");

        assert!(error.contains("voice-a.onnx.json"));
    }

    #[test]
    fn rejects_voice_when_model_file_is_missing() {
        let temp_dir = TempDir::new("resolve-no-model");
        fs::write(temp_dir.path().join("voice-a.onnx.json"), b"{}")
            .expect("config should be created");

        let error = resolve_voice_paths(temp_dir.path(), "voice-a").expect_err("voice should fail");

        assert!(error.contains("voice-a.onnx"));
    }

    #[test]
    fn cache_reuses_loaded_model_for_same_config_path() {
        let temp_dir = TempDir::new("cache-same-path");
        create_voice_pair(temp_dir.path(), "voice-a");
        let resolved =
            resolve_voice_paths(temp_dir.path(), "voice-a").expect("voice should resolve");

        let load_count = Arc::new(AtomicUsize::new(0));
        let mut cache = std::collections::HashMap::<PathBuf, Arc<FakeLoadedVoice>>::new();
        let loader = {
            let load_count = Arc::clone(&load_count);
            move |_config_path: &Path| {
                load_count.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(FakeLoadedVoice("voice-a")))
            }
        };

        let first = get_or_load_cached(&mut cache, &resolved.config_path, &loader, "voice config")
            .expect("first load should succeed");
        let second = get_or_load_cached(&mut cache, &resolved.config_path, &loader, "voice config")
            .expect("second load should succeed");

        assert_eq!(first.0, "voice-a");
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(load_count.load(Ordering::SeqCst), 1);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cache_loads_distinct_models_for_different_config_paths() {
        let temp_dir = TempDir::new("cache-different-paths");
        create_voice_pair(temp_dir.path(), "voice-a");
        create_voice_pair(temp_dir.path(), "voice-b");
        let resolved_a =
            resolve_voice_paths(temp_dir.path(), "voice-a").expect("voice a should resolve");
        let resolved_b =
            resolve_voice_paths(temp_dir.path(), "voice-b").expect("voice b should resolve");

        let load_count = Arc::new(AtomicUsize::new(0));
        let mut cache = std::collections::HashMap::<PathBuf, Arc<FakeLoadedVoice>>::new();
        let loader = {
            let load_count = Arc::clone(&load_count);
            move |config_path: &Path| {
                load_count.fetch_add(1, Ordering::SeqCst);
                let label = if config_path.ends_with("voice-a.onnx.json") {
                    "voice-a"
                } else {
                    "voice-b"
                };
                Ok(Arc::new(FakeLoadedVoice(label)))
            }
        };

        let first = get_or_load_cached(&mut cache, &resolved_a.config_path, &loader, "voice config")
            .expect("voice a should load");
        let second = get_or_load_cached(&mut cache, &resolved_b.config_path, &loader, "voice config")
            .expect("voice b should load");

        assert_eq!(first.0, "voice-a");
        assert_eq!(second.0, "voice-b");
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(load_count.load(Ordering::SeqCst), 2);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn cache_does_not_store_failed_loads() {
        let temp_dir = TempDir::new("cache-failed-loads");
        create_voice_pair(temp_dir.path(), "voice-a");
        let resolved =
            resolve_voice_paths(temp_dir.path(), "voice-a").expect("voice should resolve");

        let load_count = Arc::new(AtomicUsize::new(0));
        let mut cache = std::collections::HashMap::<PathBuf, Arc<FakeLoadedVoice>>::new();
        let loader = {
            let load_count = Arc::clone(&load_count);
            move |_config_path: &Path| {
                load_count.fetch_add(1, Ordering::SeqCst);
                Err("synthetic loader failure".to_string())
            }
        };

        let first = get_or_load_cached(&mut cache, &resolved.config_path, &loader, "voice config")
            .expect_err("first load should fail");
        let second = get_or_load_cached(&mut cache, &resolved.config_path, &loader, "voice config")
            .expect_err("second load should fail");

        assert_eq!(first, "synthetic loader failure");
        assert_eq!(second, "synthetic loader failure");
        assert_eq!(load_count.load(Ordering::SeqCst), 2);
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_uses_resolved_config_path_for_requested_voice() {
        let temp_dir = TempDir::new("cache-resolved-path");
        create_voice_pair(temp_dir.path(), "voice-a");
        create_voice_pair(temp_dir.path(), "voice-b");
        let resolved =
            resolve_voice_paths(temp_dir.path(), "voice-b").expect("voice should resolve");

        let observed_paths = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
        let mut cache = std::collections::HashMap::<PathBuf, Arc<FakeLoadedVoice>>::new();
        let loader = {
            let observed_paths = Arc::clone(&observed_paths);
            move |config_path: &Path| {
                observed_paths
                    .lock()
                    .expect("observed paths should be lockable")
                    .push(config_path.to_path_buf());
                Ok(Arc::new(FakeLoadedVoice("voice-b")))
            }
        };

        let loaded =
            get_or_load_cached(&mut cache, &resolved.config_path, &loader, "voice config")
                .expect("load should succeed");

        let paths = observed_paths
            .lock()
            .expect("observed paths should be lockable");
        assert_eq!(loaded.0, "voice-b");
        assert_eq!(paths.as_slice(), &[resolved.config_path]);
        assert_eq!(paths[0], temp_dir.path().join("voice-b.onnx.json"));
    }
}
