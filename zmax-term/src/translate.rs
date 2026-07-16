//! Machine translation — the faithful port of the Spacemacs `translate` tools
//! layer (`google-translate.el`). Translates the word under the cursor or a
//! phrase between a configurable source/target language pair, using Google's
//! keyless `gtx` endpoint (the same one the elisp layer and most CLI translate
//! tools use). Networking goes through the already-vendored blocking `ureq`
//! client, so callers run [`translate`] on a `spawn_blocking` task.

use std::sync::Mutex;

/// The active source/target language pair (`SPC x g l` sets it, `SPC x g T`
/// reverses it). `auto` source lets the endpoint detect the language.
pub struct LangPair {
    pub source: String,
    pub target: String,
}

static LANGS: Mutex<Option<LangPair>> = Mutex::new(None);

/// Current `(source, target)` languages, defaulting to `auto` -> `en`.
pub fn languages() -> (String, String) {
    LANGS
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|p| (p.source.clone(), p.target.clone())))
        .unwrap_or_else(|| ("auto".to_string(), "en".to_string()))
}

/// Set the source and target language codes (Spacemacs `SPC x g l`).
pub fn set_languages(source: &str, target: &str) {
    if let Ok(mut g) = LANGS.lock() {
        *g = Some(LangPair {
            source: source.trim().to_lowercase(),
            target: target.trim().to_lowercase(),
        });
    }
}

/// Swap source and target (Spacemacs `SPC x g T`, reverse-translate). A pending
/// `auto` source becomes `en` so the reverse direction is well-defined.
pub fn reverse_languages() {
    let (s, t) = languages();
    let s = if s == "auto" { "en".to_string() } else { s };
    set_languages(&t, &s);
}

/// Translate `text` from `source` to `target`. Returns the translated string, or
/// an error message. Blocking; call from `spawn_blocking`.
pub fn translate(text: &str, source: &str, target: &str) -> Result<String, String> {
    let q = percent_encode(text.trim());
    let url = format!(
        "https://translate.googleapis.com/translate_a/single\
         ?client=gtx&sl={source}&tl={target}&dt=t&q={q}"
    );
    let body = ureq::get(&url)
        .set("User-Agent", "zmax")
        .call()
        .map_err(|e| format!("{e}"))?
        .into_string()
        .map_err(|e| format!("read: {e}"))?;
    parse_gtx(&body).ok_or_else(|| "no translation returned".to_string())
}

/// The gtx endpoint returns `[[["translated","source",...],...],...]`. Join every
/// segment's first element to reassemble multi-sentence translations.
fn parse_gtx(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let segments = v.get(0)?.as_array()?;
    let mut out = String::new();
    for seg in segments {
        if let Some(s) = seg.get(0).and_then(|x| x.as_str()) {
            out.push_str(s);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Minimal RFC-3986 percent-encoding for the query string.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gtx_multi_segment() {
        let body = r#"[[["Hello","Hola",null,null,10],["world","mundo",null,null,3]],null,"es"]"#;
        assert_eq!(parse_gtx(body).as_deref(), Some("Helloworld"));
    }

    #[test]
    fn language_pair_set_and_reverse() {
        set_languages("EN", "fr");
        assert_eq!(languages(), ("en".to_string(), "fr".to_string()));
        reverse_languages();
        assert_eq!(languages(), ("fr".to_string(), "en".to_string()));
    }

    #[test]
    fn encodes_query() {
        assert_eq!(percent_encode("a b&c"), "a%20b%26c");
    }
}
