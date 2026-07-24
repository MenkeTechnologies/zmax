//! Passphrase encryption for the `:encrypt` / `:decrypt` builtins, using the
//! [`age`](https://age-encryption.org) format (scrypt passphrase recipient) with
//! ASCII armor so the ciphertext is text that lives happily in a buffer.
//!
//! The editor side (prompting for the passphrase behind a masked prompt, reading
//! and replacing the selection) is in `typed.rs`; this module is the pure crypto
//! so it is unit-testable without an editor.

use std::io::{Read, Write};

use age::armor::{ArmoredReader, ArmoredWriter, Format};
use age::secrecy::SecretString;

/// Encrypt `plaintext` with `passphrase`, returning an ASCII-armored age file
/// (`-----BEGIN AGE ENCRYPTED FILE-----` … ).
pub fn encrypt(plaintext: &str, passphrase: &str) -> Result<String, String> {
    let encryptor = age::Encryptor::with_user_passphrase(SecretString::from(passphrase.to_owned()));
    let mut out: Vec<u8> = Vec::new();
    let armor = ArmoredWriter::wrap_output(&mut out, Format::AsciiArmor)
        .map_err(|e| format!("armor: {e}"))?;
    let mut writer = encryptor
        .wrap_output(armor)
        .map_err(|e| format!("encrypt: {e}"))?;
    writer
        .write_all(plaintext.as_bytes())
        .map_err(|e| format!("encrypt: {e}"))?;
    // Finish the encryption stream (returns the armor writer), then the armor.
    let armor = writer.finish().map_err(|e| format!("encrypt: {e}"))?;
    armor.finish().map_err(|e| format!("armor: {e}"))?;
    String::from_utf8(out).map_err(|e| format!("encrypt: {e}"))
}

/// Decrypt an armored (or binary) age `ciphertext` with `passphrase`. Errors if
/// the passphrase is wrong or the input is not an age file.
pub fn decrypt(ciphertext: &str, passphrase: &str) -> Result<String, String> {
    // `ArmoredReader` transparently handles both armored and binary age files.
    let reader = ArmoredReader::new(ciphertext.as_bytes());
    let decryptor = age::Decryptor::new(reader).map_err(|e| format!("decrypt: {e}"))?;
    let identity = age::scrypt::Identity::new(SecretString::from(passphrase.to_owned()));
    let mut stream = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|e| format!("decrypt: {e} (wrong passphrase or not an age file)"))?;
    let mut out: Vec<u8> = Vec::new();
    stream
        .read_to_end(&mut out)
        .map_err(|e| format!("decrypt: {e}"))?;
    String::from_utf8(out).map_err(|_| "decrypt: plaintext is not valid UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let plain = "secret zmax buffer\nwith two lines\n";
        let armored = encrypt(plain, "correct horse battery staple").unwrap();
        assert!(armored.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"));
        assert_ne!(armored, plain);
        let back = decrypt(&armored, "correct horse battery staple").unwrap();
        assert_eq!(back, plain);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let armored = encrypt("hi", "right").unwrap();
        assert!(decrypt(&armored, "wrong").is_err());
    }

    #[test]
    fn decrypt_rejects_non_age() {
        assert!(decrypt("just some plain text", "x").is_err());
    }
}
