//! Save bytes as text for `localStorage`: standard base64 with padding (RFC 4648 §4).

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` as padded standard base64.
pub fn to_save_text(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let at = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = (at(0) << 16) | (at(1) << 8) | at(2);
        for i in 0..4 {
            if i <= chunk.len() {
                text.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// The bytes of padded standard base64 text; `None` for anything else (wrong length, other
/// characters, padding in the middle or more than two `=`).
pub fn from_save_text(text: &str) -> Option<Vec<u8>> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let quads = text.len() / 4;
    let mut bytes = Vec::with_capacity(quads * 3);
    for (index, quad) in text.as_chunks::<4>().0.iter().enumerate() {
        let padding = quad.iter().rev().take_while(|&&c| c == b'=').count();
        if padding > 2 || (padding > 0 && index + 1 != quads) {
            return None;
        }
        let mut n = 0u32;
        for &c in &quad[..4 - padding] {
            n = (n << 6) | u32::from(sextet(c)?);
        }
        n <<= 6 * padding as u32;
        bytes.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8][..3 - padding]);
    }
    Some(bytes)
}

fn sextet(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: [(&[u8], &str); 7] = [
        (b"", ""),
        (b"f", "Zg=="),
        (b"fo", "Zm8="),
        (b"foo", "Zm9v"),
        (b"foob", "Zm9vYg=="),
        (b"fooba", "Zm9vYmE="),
        (b"foobar", "Zm9vYmFy"),
    ];

    #[test]
    fn rfc_4648_vectors_encode_and_decode() {
        for (bytes, text) in VECTORS {
            assert_eq!(to_save_text(bytes), text);
            assert_eq!(from_save_text(text).as_deref(), Some(bytes), "{text}");
        }
    }

    #[test]
    fn every_byte_value_round_trips() {
        let bytes: Vec<u8> = (0..=255).chain((0..=255).rev()).collect();
        let text = to_save_text(&bytes);
        assert!(
            text.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"+/=".contains(&c))
        );
        assert_eq!(from_save_text(&text), Some(bytes));
    }

    #[test]
    fn damaged_text_is_rejected() {
        for bad in [
            "Zg=", "Zm9", "Z===", "Zg==Zg==", "Zg=A", "Zm9v!A==", "Zm 9v", "é123",
        ] {
            assert_eq!(from_save_text(bad), None, "{bad:?}");
        }
    }
}
