//! Coding systems — the zemacs port of the GNU Emacs `recode-region` /
//! `recode-file-name` re-decoding commands.
//!
//! When a file is read with the wrong coding system its bytes are decoded into
//! the wrong characters (the classic mojibake). Emacs fixes that without
//! re-reading the file: it encodes the text back with the coding system it was
//! *mistakenly* decoded as — reconstructing the original bytes — and decodes
//! those bytes again with the right one. That round trip is what this module is;
//! it is pure, and `encoding_rs` (already a core dependency) provides the tables.

/// Text that was decoded with the wrong coding system, re-decoded with the right
/// one. `interpreted_as` is the coding system the bytes were mistakenly read
/// with; `really_in` is what they actually were.
///
/// Unknown coding-system names, text the wrong coding system cannot represent
/// (so the original bytes cannot be reconstructed), and bytes that are not valid
/// in the target are all errors — Emacs signals in those cases rather than
/// silently mangling the buffer.
pub fn recode(text: &str, interpreted_as: &str, really_in: &str) -> Result<String, String> {
    let wrong = crate::encoding::Encoding::for_label(interpreted_as.as_bytes())
        .ok_or_else(|| format!("unknown coding system: {interpreted_as}"))?;
    let right = crate::encoding::Encoding::for_label(really_in.as_bytes())
        .ok_or_else(|| format!("unknown coding system: {really_in}"))?;
    let (bytes, _, had_errors) = wrong.encode(text);
    if had_errors {
        return Err(format!(
            "the text cannot be represented in {}",
            wrong.name()
        ));
    }
    let (decoded, _, malformed) = right.decode(&bytes);
    if malformed {
        return Err(format!("the bytes are not valid {}", right.name()));
    }
    Ok(decoded.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UTF-8 text read as Latin-1 comes out as mojibake; recoding it back the
    /// other way restores the original — the whole point of `recode-region`.
    #[test]
    fn recode_undoes_a_wrong_decoding() {
        // "héllo" in UTF-8, decoded as windows-1252, is "hÃ©llo".
        let mojibake = "hÃ©llo";
        assert_eq!(recode(mojibake, "windows-1252", "utf-8").unwrap(), "héllo");
        // …and the inverse round-trips.
        assert_eq!(recode("héllo", "utf-8", "windows-1252").unwrap(), mojibake);
    }

    /// Names that no coding system answers to, and text the source coding cannot
    /// hold, are reported rather than silently corrupting the region.
    #[test]
    fn recode_rejects_bad_input() {
        assert!(recode("x", "no-such-coding", "utf-8").is_err());
        assert!(recode("x", "utf-8", "no-such-coding").is_err());
        // A CJK character cannot be encoded in Latin-1, so the original bytes
        // cannot be reconstructed.
        assert!(recode("漢", "iso-8859-1", "utf-8").is_err());
    }
}
