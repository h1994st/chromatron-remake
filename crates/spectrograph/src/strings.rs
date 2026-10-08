use crate::{DecodeError, Image};

/// A NUL-terminated string from `__TEXT,__cstring`, decoded from MacRoman.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CString {
    pub vmaddr: u32,
    pub text: String,
}

pub fn decode_mac_roman(bytes: &[u8]) -> String {
    encoding_rs::MACINTOSH
        .decode_without_bom_handling(bytes)
        .0
        .into_owned()
}

/// Every non-empty string in `__TEXT,__cstring`, in address order.
pub fn cstrings(image: &Image) -> Result<Vec<CString>, DecodeError> {
    let section = image.section("__TEXT", "__cstring")?;
    Ok(split_cstrings(section.vmaddr, section.data))
}

/// The NUL-terminated string starting at `vmaddr`, if a NUL follows within the same section.
pub fn cstring_at(image: &Image, vmaddr: u32) -> Option<String> {
    let rest = image.tail(vmaddr)?;
    let end = rest.iter().position(|&b| b == 0)?;
    Some(decode_mac_roman(&rest[..end]))
}

fn split_cstrings(base: u32, data: &[u8]) -> Vec<CString> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, &byte) in data.iter().enumerate() {
        if byte == 0 {
            if i > start {
                out.push(CString {
                    vmaddr: base + start as u32,
                    text: decode_mac_roman(&data[start..i]),
                });
            }
            start = i + 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_mac_roman_high_bytes() {
        assert_eq!(decode_mac_roman(&[0xA9, 0x20, 0x8E]), "© é");
    }

    #[test]
    fn splits_on_nul_and_skips_empty_runs() {
        assert_eq!(
            split_cstrings(0x1000, b"ab\0\0cd\0tail"),
            vec![
                CString {
                    vmaddr: 0x1000,
                    text: "ab".into()
                },
                CString {
                    vmaddr: 0x1004,
                    text: "cd".into()
                },
            ]
        );
    }
}
