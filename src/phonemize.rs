use std::env;
use std::ffi::{c_void, CStr, CString};
use std::fmt;
use std::os::raw::c_int;
use std::path::PathBuf;
use std::ptr;
use std::sync::{Mutex, Once};

use espeak_rs_sys::{
    espeak_AUDIO_OUTPUT_AUDIO_OUTPUT_RETRIEVAL as AUDIO_OUTPUT_RETRIEVAL, espeak_Initialize,
    espeak_SetVoiceByName, espeak_TextToPhonemes, espeakINITIALIZE_DONT_EXIT,
};

const UTF8_TEXT_MODE: c_int = 1;
const IPA_NO_SEPARATOR: c_int = 0x02;
const ESPEAK_DATA_ENV: &str = "PIPER_ESPEAKNG_DATA_DIRECTORY";
const ESPEAK_DATA_DIR_NAME: &str = "espeak-ng-data";
const EDGE_PUNCT: &[char] = &[
    ',', '.', '!', '?', ';', ':', '"', '\'', '(', ')', '[', ']', '{', '}', '…', '—', '–',
];

static ESPEAK_INIT: Once = Once::new();
static ESPEAK_LOCK: Mutex<()> = Mutex::new(());

/// Word-aligned phonemize output. `phonemes` is byte-identical to the legacy
/// single-string `phonemize()` output for the same inputs; `words[].phonemes`
/// is a best-effort split and is NOT asserted byte-equal to the join of the
/// top-level string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhonemizedWords {
    pub phonemes: String,
    pub words: Vec<WordPhonemes>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordPhonemes {
    pub text: String,
    pub phonemes: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhonemizeError {
    UnsupportedLanguage(String),
    EspeakFailure(String),
}

impl fmt::Display for PhonemizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhonemizeError::UnsupportedLanguage(code) => {
                write!(f, "BCP-47 code '{code}' not supported")
            }
            PhonemizeError::EspeakFailure(msg) => f.write_str(msg),
        }
    }
}

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

/// Word-aligned phonemize. Accepts a BCP-47 language code (e.g. `en-US`,
/// `pt-BR`). Returns an empty-but-structured result for empty or
/// punctuation-only input; returns `UnsupportedLanguage` for unknown BCP-47
/// codes. Preserves byte-identity of the top-level `phonemes` string vs
/// v0.1.5 by delegating to `phonemize()` for the legacy output.
pub fn phonemize_with_words(
    text: &str,
    language: &str,
) -> Result<PhonemizedWords, PhonemizeError> {
    let input_tokens = tokenize_input(text);
    if input_tokens.is_empty() {
        return Ok(PhonemizedWords {
            phonemes: String::new(),
            words: Vec::new(),
        });
    }

    let espeak_lang = bcp47_to_espeak(language)
        .ok_or_else(|| PhonemizeError::UnsupportedLanguage(language.to_string()))?;

    let phonemes = phonemize(text, espeak_lang).map_err(PhonemizeError::EspeakFailure)?;
    let ipa_tokens: Vec<String> = phonemes
        .split_whitespace()
        .map(|t| t.to_string())
        .collect();

    let words = align_tokens(&input_tokens, &ipa_tokens);

    Ok(PhonemizedWords { phonemes, words })
}

fn tokenize_input(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|raw| raw.trim_matches(|c: char| EDGE_PUNCT.contains(&c)).to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn align_tokens(input: &[String], ipa: &[String]) -> Vec<WordPhonemes> {
    if input.len() == ipa.len() {
        return input
            .iter()
            .zip(ipa.iter())
            .map(|(t, p)| WordPhonemes {
                text: t.clone(),
                phonemes: p.clone(),
            })
            .collect();
    }

    tracing::debug!(
        event = "word_alignment_mismatch",
        input_count = input.len() as u64,
        ipa_count = ipa.len() as u64
    );

    let mut words = Vec::with_capacity(input.len());
    if ipa.len() > input.len() && !input.is_empty() {
        // More IPA tokens than input tokens: 1:1 zip, then merge trailing IPA
        // tokens into the last word's phonemes to preserve text→phoneme totality.
        let keep = input.len() - 1;
        for i in 0..keep {
            words.push(WordPhonemes {
                text: input[i].clone(),
                phonemes: ipa[i].clone(),
            });
        }
        let tail = ipa[keep..].join(" ");
        words.push(WordPhonemes {
            text: input[input.len() - 1].clone(),
            phonemes: tail,
        });
    } else {
        // Fewer IPA tokens than input tokens (e.g. eSpeak merged punctuation-
        // adjacent words). First N-1 inputs share the first N-1 IPA tokens
        // when available; the last input absorbs any remaining IPA tail so the
        // full IPA string is covered. Extra input tokens get empty phonemes.
        let pair_count = ipa.len().saturating_sub(1);
        for i in 0..pair_count {
            words.push(WordPhonemes {
                text: input[i].clone(),
                phonemes: ipa[i].clone(),
            });
        }
        for i in pair_count..input.len() - 1 {
            words.push(WordPhonemes {
                text: input[i].clone(),
                phonemes: String::new(),
            });
        }
        let tail = if ipa.is_empty() {
            String::new()
        } else {
            ipa[pair_count..].join(" ")
        };
        words.push(WordPhonemes {
            text: input[input.len() - 1].clone(),
            phonemes: tail,
        });
    }
    words
}

fn bcp47_to_espeak(lang: &str) -> Option<&'static str> {
    match lang.to_ascii_lowercase().as_str() {
        "en-us" => Some("en-us"),
        "en-gb" => Some("en-gb"),
        "en" => Some("en"),
        "pt-br" => Some("pt-br"),
        "pt-pt" | "pt" => Some("pt"),
        "de" | "de-de" => Some("de"),
        "es" | "es-es" => Some("es"),
        "fr" | "fr-fr" => Some("fr"),
        "ca" | "ca-es" => Some("ca"),
        "pl" | "pl-pl" => Some("pl"),
        "ru" | "ru-ru" => Some("ru"),
        _ => None,
    }
}

fn ensure_initialized() -> Result<(), String> {
    let mut init_error: Option<String> = None;
    ESPEAK_INIT.call_once(|| {
        // eSpeak falls back to a compile-time baked path when invoked with a
        // NULL path argument. That path is the build directory of whichever
        // machine produced the binary, which does not exist on any other
        // machine — so the released artifact cannot phonemize on a foreign
        // host unless we pass an explicit runtime path. Mirror the discovery
        // logic used by `espeak-rs` (crate vendored via piper-rs) so both
        // FFI entry points agree on the data directory.
        let path_cstr = locate_espeak_data().and_then(|p| {
            CString::new(p.to_string_lossy().as_ref()).ok()
        });
        let path_ptr = path_cstr
            .as_ref()
            .map_or(ptr::null(), |c| c.as_ptr());

        let result = unsafe {
            espeak_Initialize(
                AUDIO_OUTPUT_RETRIEVAL,
                0,
                path_ptr,
                espeakINITIALIZE_DONT_EXIT as c_int,
            )
        };
        if result < 0 {
            init_error = Some(format!(
                "espeak_Initialize failed with code {result} (data dir: {})",
                path_cstr
                    .as_ref()
                    .and_then(|c| c.to_str().ok())
                    .unwrap_or("<compile-time default>")
            ));
        }
    });
    if let Some(err) = init_error {
        return Err(err);
    }
    Ok(())
}

fn locate_espeak_data() -> Option<PathBuf> {
    // 1. Honor the repository-owned env var set by `main::discover_espeak_data_dir`.
    if let Ok(dir) = env::var(ESPEAK_DATA_ENV) {
        let p = PathBuf::from(dir);
        if p.join(ESPEAK_DATA_DIR_NAME).exists() {
            return Some(p);
        }
    }
    // 2. Current working directory (helps with `cargo run` from repo root).
    if let Ok(cwd) = env::current_dir() {
        if cwd.join(ESPEAK_DATA_DIR_NAME).exists() {
            return Some(cwd);
        }
    }
    // 3. Directory of the current executable (packaged zip layout).
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join(ESPEAK_DATA_DIR_NAME).exists() {
                return Some(dir.to_path_buf());
            }
            let runtime = dir.join("espeak-runtime");
            if runtime.join(ESPEAK_DATA_DIR_NAME).exists() {
                return Some(runtime);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcp47_map_covers_directive_languages() {
        assert_eq!(bcp47_to_espeak("en-US"), Some("en-us"));
        assert_eq!(bcp47_to_espeak("EN-us"), Some("en-us"));
        assert_eq!(bcp47_to_espeak("pt-BR"), Some("pt-br"));
        assert_eq!(bcp47_to_espeak("pt"), Some("pt"));
        assert_eq!(bcp47_to_espeak("de"), Some("de"));
        assert_eq!(bcp47_to_espeak("zz-ZZ"), None);
        assert_eq!(bcp47_to_espeak(""), None);
    }

    #[test]
    fn tokenize_input_strips_edge_punctuation_and_keeps_contractions() {
        let tokens = tokenize_input("Hello, I'd like it.");
        assert_eq!(
            tokens,
            vec![
                "Hello".to_string(),
                "I'd".to_string(),
                "like".to_string(),
                "it".to_string(),
            ]
        );
    }

    #[test]
    fn tokenize_input_empty_or_punct_only_returns_no_tokens() {
        assert!(tokenize_input("").is_empty());
        assert!(tokenize_input("   ").is_empty());
        assert!(tokenize_input("... !! ?? ,,").is_empty());
    }

    #[test]
    fn align_tokens_one_to_one_when_counts_match() {
        let input = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let ipa = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let words = align_tokens(&input, &ipa);
        assert_eq!(words.len(), 3);
        assert_eq!(words[0].text, "a");
        assert_eq!(words[0].phonemes, "A");
        assert_eq!(words[2].phonemes, "C");
    }

    #[test]
    fn align_tokens_merges_trailing_ipa_into_last_entry_when_ipa_has_more() {
        let input = vec!["x".to_string(), "y".to_string()];
        let ipa = vec!["X".to_string(), "Y".to_string(), "Z".to_string()];
        let words = align_tokens(&input, &ipa);
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].phonemes, "X");
        assert_eq!(words[1].text, "y");
        assert_eq!(words[1].phonemes, "Y Z");
    }

    #[test]
    fn align_tokens_absorbs_tail_into_last_entry_when_ipa_has_fewer() {
        // Example mirrors "Good morning, everyone" → 3 input vs 2 IPA tokens.
        let input = vec!["Good".to_string(), "morning".to_string(), "everyone".to_string()];
        let ipa = vec!["ɡˈʊd".to_string(), "mˈɔːɹnɪŋˈɛvɹɪwˌʌn".to_string()];
        let words = align_tokens(&input, &ipa);
        assert_eq!(words.len(), 3);
        assert_eq!(words[0].phonemes, "ɡˈʊd");
        assert_eq!(words[1].phonemes, "");
        assert_eq!(words[2].phonemes, "mˈɔːɹnɪŋˈɛvɹɪwˌʌn");
    }

    #[test]
    fn phonemize_with_words_empty_input_returns_empty_structure() {
        let result = phonemize_with_words("", "en-US").expect("empty is legal");
        assert_eq!(result.phonemes, "");
        assert!(result.words.is_empty());
    }

    #[test]
    fn phonemize_with_words_whitespace_only_input_returns_empty_structure() {
        let result = phonemize_with_words("   \t  ", "en-US").expect("ws-only is legal");
        assert_eq!(result.phonemes, "");
        assert!(result.words.is_empty());
    }

    #[test]
    fn phonemize_with_words_punct_only_input_returns_empty_structure() {
        let result = phonemize_with_words("... !! ??", "en-US").expect("punct-only is legal");
        assert_eq!(result.phonemes, "");
        assert!(result.words.is_empty());
    }

    #[test]
    fn phonemize_with_words_unknown_bcp47_errors_unsupported_language() {
        let err = phonemize_with_words("hello", "zz-ZZ").expect_err("unknown lang should fail");
        match err {
            PhonemizeError::UnsupportedLanguage(code) => assert_eq!(code, "zz-ZZ"),
            other => panic!("expected UnsupportedLanguage, got {other:?}"),
        }
    }
}
