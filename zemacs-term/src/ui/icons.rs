//! Nerd-font glyph icons for the UI (file tree, bufferline tabs, …).
//!
//! These require a patched "Nerd Font" in the terminal. Glyphs are chosen from
//! the common devicons/seti ranges so most Nerd Fonts render them.

/// Folder glyph for the file tree (open vs. closed).
pub fn folder_icon(expanded: bool) -> char {
    if expanded {
        '\u{f07c}' // nf-fa-folder_open
    } else {
        '\u{f07b}' // nf-fa-folder
    }
}

/// A filetype icon for a file name, chosen by extension (falls back to a few
/// well-known basenames, then a generic file glyph).
pub fn file_icon(name: &str) -> char {
    // Special-case some well-known basenames first.
    match name.to_ascii_lowercase().as_str() {
        "cargo.toml" | "cargo.lock" => return '\u{e7a8}', // rust
        ".gitignore" | ".gitattributes" | ".gitmodules" => return '\u{e702}', // git
        "dockerfile" => return '\u{f308}',                // docker
        "makefile" => return '\u{e673}',
        "license" | "license.md" | "license.txt" => return '\u{f0fc}',
        "readme" | "readme.md" => return '\u{f48a}',
        _ => {}
    }

    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "rs" => '\u{e7a8}',                                // rust
        "py" | "pyi" | "pyc" => '\u{e606}',                // python
        "js" | "mjs" | "cjs" => '\u{e74e}',                // javascript
        "jsx" => '\u{e7ba}',                               // react
        "ts" => '\u{e628}',                                // typescript
        "tsx" => '\u{e7ba}',                               // react
        "go" => '\u{e627}',                                // go
        "c" | "h" => '\u{e61e}',                           // c
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => '\u{e61d}', // c++
        "cs" => '\u{f81a}',                                // c#
        "java" => '\u{e738}',
        "kt" | "kts" => '\u{e634}', // kotlin
        "rb" => '\u{e739}',         // ruby
        "php" => '\u{e73d}',
        "swift" => '\u{e755}',
        "lua" => '\u{e620}',
        "vim" => '\u{e62b}',
        "el" | "lisp" | "clj" | "cljs" => '\u{e779}', // lisp/clojure
        "hs" => '\u{e777}',                           // haskell
        "scala" => '\u{e737}',
        "html" | "htm" => '\u{e736}',
        "css" => '\u{e749}',
        "scss" | "sass" => '\u{e603}',
        "json" | "jsonc" => '\u{e60b}',
        "toml" => '\u{e6b2}',
        "yaml" | "yml" => '\u{e615}',
        "xml" => '\u{e619}',
        "md" | "markdown" => '\u{e73e}',
        "sh" | "bash" | "zsh" | "fish" | "zwc" => '\u{e795}', // shell
        "ps1" => '\u{e795}',
        "sql" => '\u{e706}',
        "txt" | "text" | "log" => '\u{f15c}',
        "pdf" => '\u{f1c1}',
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" => '\u{f1c5}', // image
        "zip" | "tar" | "gz" | "xz" | "zst" | "bz2" | "7z" | "rar" => '\u{f1c6}',      // archive
        "lock" => '\u{f023}',
        "" => '\u{f016}', // extensionless → plain file
        _ => '\u{f15b}',  // generic file
    }
}
