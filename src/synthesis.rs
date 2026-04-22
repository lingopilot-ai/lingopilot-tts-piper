use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use piper_rs::Piper;

type SynthHandle = Arc<Mutex<Piper>>;
type SynthLoader = dyn Fn(&Path, &Path) -> Result<SynthHandle, String> + Send + Sync;

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
        Self::with_loader(load_piper)
    }

    fn with_loader<L>(loader: L) -> Self
    where
        L: Fn(&Path, &Path) -> Result<SynthHandle, String> + Send + Sync + 'static,
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
        voice_model_path: &Path,
        voice_config_path: &Path,
        speed: f32,
    ) -> Result<SynthResult, String> {
        let piper = self.get_or_load(voice_model_path, voice_config_path)?;
        synthesize_with_piper(&piper, text, speed)
    }

    fn get_or_load(
        &mut self,
        model_path: &Path,
        config_path: &Path,
    ) -> Result<SynthHandle, String> {
        let key = config_path;
        if let Some(value) = self.models.get(key) {
            tracing::debug!(
                event = "synth_cache_hit",
                cache_key = key.display().to_string()
            );
            return Ok(Arc::clone(value));
        }

        tracing::debug!(
            event = "synth_cache_miss",
            cache_key = key.display().to_string()
        );
        let value = (self.loader)(model_path, config_path).map_err(|e| {
            tracing::debug!(
                event = "synth_cache_load_failed",
                cache_key = key.display().to_string(),
                detail = e.as_str()
            );
            e
        })?;
        self.models.insert(key.to_path_buf(), Arc::clone(&value));
        Ok(value)
    }
}

fn load_piper(model_path: &Path, config_path: &Path) -> Result<SynthHandle, String> {
    let piper = Piper::new(model_path, config_path)
        .map_err(|e| format!("Failed to load Piper voice: {}", e))?;
    Ok(Arc::new(Mutex::new(piper)))
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

fn synthesize_with_piper(
    piper: &Mutex<Piper>,
    text: &str,
    speed: f32,
) -> Result<SynthResult, String> {
    let mut guard = piper
        .lock()
        .map_err(|_| "Piper cache lock poisoned".to_string())?;

    // In Piper, length_scale is the inverse of speed: lower length_scale
    // produces faster speech.
    let length_scale = if (speed - 1.0).abs() < f32::EPSILON {
        None
    } else {
        Some(1.0 / speed)
    };

    let (samples, sample_rate) = guard
        .create(text, false, None, length_scale, None, None)
        .map_err(|e| format!("Synthesis failed: {}", e))?;

    if samples.is_empty() {
        return Err("Synthesis produced no audio".to_string());
    }

    let pcm16: Vec<i16> = samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();

    Ok(SynthResult { pcm16, sample_rate })
}


#[cfg(test)]
mod tests {
    use super::{validate_espeak_data_dir, validate_voice_file};
    use std::fs;
    use std::path::{Path, PathBuf};
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

}
