//! Four small spacemacs layers that are pure editor behaviour rather than an
//! integration: `+misc/nav-flash`, `+vim/vim-empty-lines`, `+fun/selectric` and
//! `+fonts/unicode-fonts`.
//!
//! They share nothing but their size, so they share a module instead of four
//! near-empty ones. Each section below states what the emacs package did and how
//! the terminal reproduces it.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use zmax_view::DocumentId;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/* ── nav-flash ──────────────────────────────────────────────────────────── */
//
// nav-flash.el briefly highlights the line point landed on after a navigation
// command, so the eye can find the cursor again after a jump. Emacs hooks it to
// a list of commands; zmax detects the jump itself: the render path records the
// cursor line per view and, when it moves by more than one line in a single
// frame, arms a flash. That covers every jump the emacs package advises
// (searches, `goto-line`, imenu, xref, window switches) and deliberately does
// not fire for `j`/`k` stepping, which is the distinction the package's
// exclude-list exists to draw.

/// Whether nav-flash is on. Off until `:nav-flash-mode` turns it on, matching a
/// layer that is not enabled by default.
static NAV_FLASH: AtomicBool = AtomicBool::new(false);
/// The armed flash: which document/line, and when it started.
static FLASH: Mutex<Option<(DocumentId, usize, Instant)>> = Mutex::new(None);
/// Last cursor line seen per view, for jump detection.
static LAST_LINE: Mutex<Vec<(zmax_view::ViewId, usize)>> = Mutex::new(Vec::new());

/// `nav-flash-delay` — how long the highlight stays up. nav-flash.el's default.
const FLASH_DELAY: Duration = Duration::from_millis(400);

/// Toggle nav-flash; returns the new state.
pub fn toggle_nav_flash() -> bool {
    let on = !NAV_FLASH.load(Ordering::Relaxed);
    NAV_FLASH.store(on, Ordering::Relaxed);
    if !on {
        *lock(&FLASH) = None;
    }
    on
}

/// Whether nav-flash is on.
pub fn nav_flash_enabled() -> bool {
    NAV_FLASH.load(Ordering::Relaxed)
}

/// Record where the cursor is for `view_key`, arming a flash when it moved far
/// enough to count as a jump. Called once per frame from the render path.
pub fn note_cursor(view_key: zmax_view::ViewId, doc: DocumentId, line: usize) {
    if !nav_flash_enabled() {
        return;
    }
    let mut seen = lock(&LAST_LINE);
    match seen.iter_mut().find(|(k, _)| *k == view_key) {
        Some((_, prev)) => {
            let jumped = line.abs_diff(*prev) > 1;
            *prev = line;
            drop(seen);
            if jumped {
                arm(doc, line);
            }
        }
        None => seen.push((view_key, line)),
    }
}

/// Start a flash on `line` and schedule the redraw that clears it.
fn arm(doc: DocumentId, line: usize) {
    *lock(&FLASH) = Some((doc, line, Instant::now()));
    // The editor only repaints on input, so without this the highlight would
    // linger until the next keypress. A one-shot thread wakes the main loop once
    // the flash has expired; the callback does nothing but cause the redraw.
    std::thread::spawn(|| {
        std::thread::sleep(FLASH_DELAY + Duration::from_millis(20));
        crate::job::dispatch_blocking(|_editor, _compositor| {});
    });
}

/// The line currently flashing in `doc`, if any. Expired flashes are cleared
/// here, so the render path only ever sees a live one.
pub fn flashing_line(doc: DocumentId) -> Option<usize> {
    if !nav_flash_enabled() {
        return None;
    }
    let mut guard = lock(&FLASH);
    match *guard {
        Some((d, line, at)) if d == doc && at.elapsed() < FLASH_DELAY => Some(line),
        Some((_, _, at)) if at.elapsed() >= FLASH_DELAY => {
            *guard = None;
            None
        }
        _ => None,
    }
}

/* ── vim-empty-lines ────────────────────────────────────────────────────── */
//
// vim-empty-lines-mode draws vim's `~` end-of-buffer markers as buffer text
// rather than fringe bitmaps. zmax already renders those rows from vim's
// `fillchars` `eob:` item, which defaults to a space; this mode is the toggle
// that turns the marker on without the user having to `:set fillchars=eob:~`,
// and the render path consults it as a fallback when `fillchars` says nothing.

/// Whether the `~` end-of-buffer markers are on.
static EMPTY_LINES: AtomicBool = AtomicBool::new(false);

/// Toggle the `~` markers; returns the new state.
pub fn toggle_empty_lines() -> bool {
    let on = !EMPTY_LINES.load(Ordering::Relaxed);
    EMPTY_LINES.store(on, Ordering::Relaxed);
    on
}

/// Whether the mode is on.
pub fn empty_lines_enabled() -> bool {
    EMPTY_LINES.load(Ordering::Relaxed)
}

/// The end-of-buffer fill character this mode asks for, or `None` when it is
/// off (in which case `fillchars` alone decides).
pub fn empty_lines_char() -> Option<char> {
    empty_lines_enabled().then_some('~')
}

/* ── selectric ──────────────────────────────────────────────────────────── */
//
// selectric-mode plays IBM Selectric samples on every keystroke: a key click, a
// distinct sound for return, and the margin bell. Emacs ships the WAVs and calls
// `play-sound-file`. zmax has no bundled audio assets, so it synthesises the two
// samples once into `~/.cache/zmax/selectric/` — the click is a short burst of
// decaying noise, the return a lower thump followed by the bell tone — and plays
// them with whichever of `afplay`/`paplay`/`aplay`/`play` the machine has,
// detached so the editor never waits on audio.

/// Whether selectric-mode is on.
static SELECTRIC: AtomicBool = AtomicBool::new(false);
/// Nanosecond timestamp of the last sound, so a held key does not fork a player
/// process per repeat.
static LAST_SOUND: AtomicU64 = AtomicU64::new(0);
/// Shortest gap between two sounds.
const SOUND_GAP: Duration = Duration::from_millis(35);

/// Toggle selectric-mode. Turning it on writes the samples if they are missing;
/// a failure to do so is returned so the command can report it rather than
/// silently enabling a mode that makes no sound.
pub fn toggle_selectric() -> Result<bool, String> {
    if SELECTRIC.load(Ordering::Relaxed) {
        SELECTRIC.store(false, Ordering::Relaxed);
        return Ok(false);
    }
    if player().is_none() {
        return Err("selectric: no audio player found (afplay, paplay, aplay or play)".into());
    }
    ensure_samples()?;
    SELECTRIC.store(true, Ordering::Relaxed);
    Ok(true)
}

/// Whether selectric-mode is on.
pub fn selectric_enabled() -> bool {
    SELECTRIC.load(Ordering::Relaxed)
}

/// Play the sound for one key press. Called for every key the editor reads.
pub fn selectric_key(key: &zmax_view::input::KeyEvent) {
    use zmax_view::keyboard::KeyCode;
    if !selectric_enabled() {
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let last = LAST_SOUND.load(Ordering::Relaxed);
    if now.saturating_sub(last) < SOUND_GAP.as_nanos() as u64 {
        return;
    }
    LAST_SOUND.store(now, Ordering::Relaxed);

    // Return gets the carriage-return sound; everything else the key click,
    // which is how selectric-mode splits its samples.
    let sample = match key.code {
        KeyCode::Enter => "return.wav",
        _ => "click.wav",
    };
    let Some(path) = sample_dir().map(|d| d.join(sample)) else {
        return;
    };
    let Some(player) = player() else { return };
    let _ = Command::new(player)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// The first available command-line audio player.
fn player() -> Option<&'static str> {
    ["afplay", "paplay", "aplay", "play"]
        .into_iter()
        .find(|p| crate::sm::have(p))
}

/// Where the synthesised samples live.
fn sample_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("zmax").join("selectric"))
}

/// Write the two samples if they are not already on disk.
fn ensure_samples() -> Result<(), String> {
    let dir = sample_dir().ok_or("selectric: no cache directory (set $HOME)")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("selectric: {}: {e}", dir.display()))?;
    let click = dir.join("click.wav");
    if !click.is_file() {
        write_wav(&click, &click_samples())?;
    }
    let ret = dir.join("return.wav");
    if !ret.is_file() {
        write_wav(&ret, &return_samples())?;
    }
    Ok(())
}

/// Sample rate of the synthesised WAVs.
const SAMPLE_RATE: u32 = 22_050;

/// A typebar strike: 25 ms of noise under a sharp exponential decay.
fn click_samples() -> Vec<i16> {
    let n = (SAMPLE_RATE as f64 * 0.025) as usize;
    // A tiny LCG keeps the noise deterministic (the same click every time) and
    // avoids pulling in a random-number dependency.
    let mut state: u32 = 0x2545_F491;
    (0..n)
        .map(|i| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (state >> 16) as i32 - 32_768 / 2;
            let decay = (-(i as f64) / (n as f64 * 0.18)).exp();
            (noise as f64 * 0.35 * decay) as i16
        })
        .collect()
}

/// Carriage return: the same strike, then the margin bell — a 1 kHz tone with a
/// slow decay, which is the sound selectric-mode plays on newline.
fn return_samples() -> Vec<i16> {
    let mut out = click_samples();
    let n = (SAMPLE_RATE as f64 * 0.35) as usize;
    out.extend((0..n).map(|i| {
        let t = i as f64 / SAMPLE_RATE as f64;
        let decay = (-t / 0.12).exp();
        (8_000.0 * decay * (2.0 * std::f64::consts::PI * 1_000.0 * t).sin()) as i16
    }));
    out
}

/// Write 16-bit mono PCM as a canonical 44-byte-header WAV.
fn write_wav(path: &std::path::Path, samples: &[i16]) -> Result<(), String> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM header size
    out.extend_from_slice(&1u16.to_le_bytes()); // format: PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // channels: mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    let mut file =
        std::fs::File::create(path).map_err(|e| format!("selectric: {}: {e}", path.display()))?;
    file.write_all(&out)
        .map_err(|e| format!("selectric: {}: {e}", path.display()))
}

/* ── unicode-fonts ──────────────────────────────────────────────────────── */
//
// unicode-fonts.el maps each Unicode block to the best installed font, because a
// GUI Emacs picks fonts itself. A terminal does not: the emulator owns font
// selection and its fallback chain, and no escape sequence lets an application
// choose a font per block. What the editor *can* do is tell you what your
// terminal's chain actually covers, which is the question the layer exists to
// answer, so this renders a sample sheet: one line per block with characters
// from it, plus the width zmax computes for each sample. A block that shows
// boxes or whose columns do not line up is one your terminal font does not
// cover, and the fix is a terminal-font change rather than an editor setting.

/// Blocks sampled by [`unicode_sample_sheet`]: name, and characters from it.
#[rustfmt::skip]
const SAMPLE_BLOCKS: &[(&str, &str)] = &[
    ("Basic Latin",                  "ABCabc012 !@#$%^&*()"),
    ("Latin-1 Supplement",           "ÀÇÉÑÖØÜßàçéñöøüÿ"),
    ("Latin Extended-A",             "ĀĆČĐĒĖĘĞİŁŃŐŒŚŠŰŸŹŻŽ"),
    ("Latin Extended-B",             "ƁƆƉƐƩƪǍǏǑǓǢǼȘȚ"),
    ("IPA Extensions",               "ɑɒɔəɛɜɡɪʃʊʌʒʔ"),
    ("Spacing Modifier Letters",     "ʰʲʳʷˆˇˈˌː˚˜"),
    ("Combining Diacritical Marks",  "a\u{300}e\u{301}i\u{302}o\u{303}u\u{308}n\u{327}"),
    ("Greek and Coptic",             "ΑΒΓΔΘΛΞΠΣΦΨΩαβγδθλξπσφψω"),
    ("Cyrillic",                     "АБВГДЖЗИЙЛПФЦЧШЩЪЫЬЭЮЯ"),
    ("Hebrew",                       "אבגדהוזחטיכלמנסעפצקרשת"),
    ("Arabic",                       "ابتثجحخدذرزسشصضطظعغفقكلمنهوي"),
    ("Devanagari",                   "अआइईउऊएऐओऔकखगघङचछजझ"),
    ("Thai",                         "กขคงจฉชซญฎฏฐฑฒณดตถทธน"),
    ("Hiragana",                     "あいうえおかきくけこさしすせそ"),
    ("Katakana",                     "アイウエオカキクケコサシスセソ"),
    ("CJK Unified Ideographs",       "漢字日本語中文简体繁體"),
    ("Hangul Syllables",             "가나다라마바사아자차카타파하"),
    ("General Punctuation",          "‐–—‘’“”†‡•…‰′″‹›⁄"),
    ("Currency Symbols",             "₠₡₣₤₥₦₧₨₩₪₫€₭₮₯₰₱₲₳₴₵₹₺₽"),
    ("Letterlike Symbols",           "℀℃№™Ω℮ⅅⅆⅇⅈⅉ"),
    ("Number Forms",                 "⅐⅑⅒⅓⅔⅕⅖⅗⅘⅙⅚⅛⅜⅝⅞ⅠⅤⅩⅬⅭⅮⅯ"),
    ("Arrows",                       "←↑→↓↔↕↖↗↘↙↚↛↞↠↢↣↦↩↪⇐⇒⇔⇦⇧⇨⇩"),
    ("Mathematical Operators",       "∀∂∃∅∇∈∉∏∑−∕∗∘√∝∞∠∧∨∩∪∫≈≠≡≤≥⊂⊃⊆⊇⊕⊗⊥⋅"),
    ("Miscellaneous Technical",      "⌀⌂⌐⌘⌚⌛⌥⌦⌫⎋⏎⏏⏚⏛"),
    ("Box Drawing",                  "─│┌┐└┘├┤┬┴┼═║╔╗╚╝╠╣╦╩╬"),
    ("Block Elements",               "▀▁▂▃▄▅▆▇█▉▊▋▌▍▎▏░▒▓"),
    ("Geometric Shapes",             "■□▪▫▬▲△▶▷▼▽◀◁◆◇○●◐◑◔◕◘◙"),
    ("Miscellaneous Symbols",        "☀☁☂☃★☆☎☑☒☕☘☠☢☣☯☸♀♂♠♣♥♦♪♫"),
    ("Dingbats",                     "✂✈✉✌✎✓✔✕✖✗✘✚✝✠✦✧✨❄❌❤➔➜➤"),
    ("Braille Patterns",             "⠁⠃⠉⠙⠑⠋⠛⠓⠊⠚⡀⡄⡆⡇⣿"),
    ("Powerline / Private Use",      "\u{e0b0}\u{e0b1}\u{e0b2}\u{e0b3}\u{e0a0}\u{e0a1}\u{e0a2}"),
    ("Emoticons",                    "😀😁😂😊😍😎😱🙂🙃"),
    ("Transport and Map Symbols",    "🚀🚁🚂🚃🚄🚅🚑🚒🚓"),
    ("Supplemental Symbols",         "🤔🤖🤝🤞🥁🥂🦀🦆🧠"),
];

/// The sample sheet: one line per block, with the characters and the total
/// display width zmax computes for them. Compare the printed width against how
/// many columns the row really occupies on screen — they disagree exactly when
/// the terminal substituted a glyph of a different width.
pub fn unicode_sample_sheet() -> String {
    use zmax_core::unicode::width::UnicodeWidthStr;
    let name_width = SAMPLE_BLOCKS
        .iter()
        .map(|(n, _)| n.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = crate::sm::heading("Unicode block coverage — samples drawn by your terminal font");
    out.push_str(
        "Each row is one block. A box, a blank, or a row whose columns do not line\n\
         up means your terminal font does not cover that block: the fix is a font\n\
         with wider coverage (or a fallback chain) in the terminal, not in zmax.\n\n",
    );
    for (name, samples) in SAMPLE_BLOCKS {
        out.push_str(&format!(
            "{name:<name_width$}  {samples}   [{} chars, {} cols]\n",
            samples.chars().count(),
            UnicodeWidthStr::width(*samples),
        ));
    }
    out
}

/// What zmax knows about one character: codepoint, the sampled block it belongs
/// to (when it is one of the sampled ones), and the display width zmax assigns
/// it. `unicode-fonts` answers "which font draws this?"; in a terminal the only
/// answerable half is "how wide does the editor think it is?", which is what
/// decides whether the line ends up misaligned.
pub fn describe_char(ch: char) -> String {
    use zmax_core::unicode::width::UnicodeWidthChar;
    let block = SAMPLE_BLOCKS
        .iter()
        .find(|(_, samples)| samples.chars().any(|c| c == ch))
        .map(|(name, _)| *name)
        .unwrap_or("(not in the sampled blocks)");
    format!(
        "U+{:04X} {ch:?}  block {block}  width {}",
        ch as u32,
        UnicodeWidthChar::width(ch).unwrap_or(0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_lines_toggle_reports_its_char() {
        assert!(toggle_empty_lines());
        assert_eq!(empty_lines_char(), Some('~'));
        assert!(!toggle_empty_lines());
        assert_eq!(empty_lines_char(), None);
    }

    #[test]
    fn a_wav_has_a_44_byte_header_and_two_bytes_per_sample() {
        let dir = std::env::temp_dir().join("zmax-selectric-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.wav");
        let samples = click_samples();
        write_wav(&path, &samples).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(bytes.len(), 44 + samples.len() * 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_click_is_short_and_bounded() {
        let s = click_samples();
        assert_eq!(s.len(), (SAMPLE_RATE as f64 * 0.025) as usize);
        assert!(s.iter().all(|v| v.abs() < 20_000));
    }

    #[test]
    fn every_sampled_block_has_samples_and_the_sheet_lists_them_all() {
        assert!(SAMPLE_BLOCKS.iter().all(|(n, s)| !n.is_empty() && !s.is_empty()));
        let sheet = unicode_sample_sheet();
        for (name, _) in SAMPLE_BLOCKS {
            assert!(sheet.contains(name), "{name} missing from the sheet");
        }
    }

    #[test]
    fn describe_char_names_the_block_and_the_width() {
        assert!(describe_char('漢').contains("CJK Unified Ideographs"));
        assert!(describe_char('漢').contains("width 2"));
        assert!(describe_char('A').contains("U+0041"));
    }
}
