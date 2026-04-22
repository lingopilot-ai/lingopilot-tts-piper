use std::ffi::{c_void, CStr, CString};
use std::os::raw::c_int;
use std::ptr;
use std::sync::{Mutex, Once};

use espeak_rs_sys::{
    espeak_AUDIO_OUTPUT_AUDIO_OUTPUT_RETRIEVAL as AUDIO_OUTPUT_RETRIEVAL, espeak_Initialize,
    espeak_SetVoiceByName, espeak_TextToPhonemes,
};

const UTF8_TEXT_MODE: c_int = 1;
const IPA_NO_SEPARATOR: c_int = 0x02;

static ESPEAK_INIT: Once = Once::new();
static ESPEAK_LOCK: Mutex<()> = Mutex::new(());

/// Produce UTF-8 IPA phonemes for `text` in the given `language` (eSpeak-NG name,
/// e.g. `en`, `pt-br`, `de`). eSpeak-NG is not thread-safe, so calls are
/// serialized with a process-wide mutex.
pub fn phonemize(text: &str, language: &str) -> Result<String, String> {
    let _guard = ESPEAK_LOCK
        .lock()
        .map_err(|_| "eSpeak phonemize lock poisoned".to_string())?;

    ensure_initialized()?;

    let voice_name = CString::new(language)
        .map_err(|_| format!("language '{language}' contains an interior NUL byte"))?;
    let status = unsafe { espeak_SetVoiceByName(voice_name.as_ptr()) };
    if status != 0 {
        return Err(format!(
            "eSpeak rejected language '{language}' (code {status})"
        ));
    }

    let text_c = CString::new(text)
        .map_err(|_| "text contains an interior NUL byte".to_string())?;
    let mut ptr: *const c_void = text_c.as_ptr() as *const c_void;

    let mut out = String::new();
    loop {
        let chunk_ptr =
            unsafe { espeak_TextToPhonemes(&mut ptr, UTF8_TEXT_MODE, IPA_NO_SEPARATOR) };
        if chunk_ptr.is_null() {
            break;
        }
        let chunk = unsafe { CStr::from_ptr(chunk_ptr) };
        match chunk.to_str() {
            Ok(s) => out.push_str(s),
            Err(_) => return Err("eSpeak emitted non-UTF-8 phoneme bytes".to_string()),
        }
        if ptr.is_null() {
            break;
        }
    }

    Ok(out)
}

fn ensure_initialized() -> Result<(), String> {
    let mut init_error: Option<String> = None;
    ESPEAK_INIT.call_once(|| {
        let result = unsafe {
            espeak_Initialize(
                AUDIO_OUTPUT_RETRIEVAL,
                0,
                ptr::null(),
                0,
            )
        };
        if result < 0 {
            init_error = Some(format!("espeak_Initialize failed with code {result}"));
        }
    });
    if let Some(err) = init_error {
        return Err(err);
    }
    Ok(())
}
