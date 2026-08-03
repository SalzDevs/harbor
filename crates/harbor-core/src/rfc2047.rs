//! RFC 2047 encoded-word decoding for headers.

/// Decode RFC 2047 encoded words (`=?charset?B?payload?=`) found anywhere in
/// `input`. Whitespace between two adjacent encoded words is dropped, per
/// RFC 2047 §6.2. Text without encoded words is returned unchanged.
pub fn decode_encoded_words(input: &str) -> String {
    if !input.contains("=?") {
        return input.to_string();
    }

    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    let mut prev_was_word = false;

    while i < bytes.len() {
        if bytes[i] == b'=' && i + 1 < bytes.len() && bytes[i + 1] == b'?' {
            if let Some((consumed, word)) = decode_word(bytes, i) {
                // RFC 2047 §6.2: LWSP separating two encoded words is dropped.
                if prev_was_word {
                    while out.ends_with([' ', '\t', '\r', '\n']) {
                        out.pop();
                    }
                }
                out.push_str(&word);
                i += consumed;
                prev_was_word = true;
                continue;
            }
        }

        let ch = bytes[i] as char;
        out.push(ch);
        if !ch.is_whitespace() {
            prev_was_word = false;
        }
        i += 1;
    }

    out
}

/// Try to parse an encoded word starting at `start` (which must point at `=?`).
/// Returns the number of consumed bytes and the decoded text, or `None` if the
/// input is not a well-formed encoded word.
fn decode_word(bytes: &[u8], start: usize) -> Option<(usize, String)> {
    let body = &bytes[start + 2..];

    // Charset ends at the first '?'.
    let charset_end = body.iter().position(|&b| b == b'?')?;
    if charset_end == 0 {
        return None;
    }
    let charset = String::from_utf8_lossy(&body[..charset_end]).to_string();

    // Encoding char follows.
    let encoding = *body.get(charset_end + 1)?;
    if !matches!(encoding, b'B' | b'b' | b'Q' | b'q') {
        return None;
    }

    // Layout: `=?` charset `?` <enc> `?` payload `?=`
    // body[charset_end] is the charset terminator `?`,
    // body[charset_end+1] is the encoding char,
    // body[charset_end+2] is the separator `?`, payload starts after it.
    let payload_start = charset_end + 3;
    let rest = &body[payload_start..];
    let delim = rest.windows(2).position(|w| w == b"?=")?;

    let payload = &body[payload_start..payload_start + delim];
    let decoded = match encoding {
        b'B' | b'b' => base64_payload(payload)?,
        _ => quoted_printable_payload(payload),
    };

    // Consumed: `=?` (2) + charset (charset_end) + `?` (1) + enc (1) +
    // `?` (1) + payload (delim) + `=` (2).
    let consumed = 2 + charset_end + 1 + 1 + 1 + delim + 2;
    Some((consumed, decode_bytes(&charset, &decoded)))
}

fn base64_payload(payload: &[u8]) -> Option<Vec<u8>> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    STANDARD.decode(payload).ok().filter(|v| !v.is_empty())
}

fn quoted_printable_payload(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len());
    let mut i = 0;
    while i < payload.len() {
        let b = payload[i];
        match b {
            b'_' => {
                out.push(b' ');
                i += 1;
            }
            b'=' if i + 2 < payload.len() => {
                if let (Some(hi), Some(lo)) = (hex_val(payload[i + 1]), hex_val(payload[i + 2])) {
                    out.push((hi << 4) | lo);
                    i += 3;
                } else {
                    out.push(b'=');
                    i += 1;
                }
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Decode payload bytes as text in `charset`.
fn decode_bytes(charset: &str, data: &[u8]) -> String {
    // Some encoders append a language suffix (e.g. `utf-8*pt`); drop it.
    let name = charset
        .split('*')
        .next()
        .unwrap_or(charset)
        .trim()
        .to_ascii_lowercase();
    match name.as_str() {
        "utf-8" | "utf8" | "us-ascii" | "ascii" | "" => String::from_utf8_lossy(data).to_string(),
        "iso-8859-1" | "latin1" | "latin-1" | "iso8859-1" | "l1" => {
            data.iter().map(|&b| b as char).collect()
        }
        "windows-1252" | "cp1252" | "x-cp1252" => data.iter().map(|&b| decode_cp1252(b)).collect(),
        _ => String::from_utf8_lossy(data).to_string(),
    }
}

fn decode_cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž',
        '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}',
        'ž', 'Ÿ',
    ];
    match b {
        b' '..=b'~' => b as char,
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_without_encoded_words() {
        assert_eq!(decode_encoded_words("no subject info"), "no subject info");
        assert_eq!(decode_encoded_words(""), "");
    }

    #[test]
    fn decodes_utf8_base64() {
        assert_eq!(decode_encoded_words("=?UTF-8?B?w4Fpb3M=?="), "Áios");
    }

    #[test]
    fn decodes_utf8_quoted_printable() {
        assert_eq!(
            decode_encoded_words("=?UTF-8?Q?Ol=C3=A1,_mundo?="),
            "Olá, mundo"
        );
    }

    #[test]
    fn adjacent_encoded_words_drop_separator_whitespace() {
        assert_eq!(
            decode_encoded_words("=?UTF-8?Q?Karim_Benzema_-_Class_Strik?= =?UTF-8?Q?er_2015?="),
            "Karim Benzema - Class Striker 2015"
        );
    }

    #[test]
    fn encoded_word_followed_by_plain_text_keeps_space() {
        assert_eq!(
            decode_encoded_words("=?UTF-8?Q?Hello?= world"),
            "Hello world"
        );
    }

    #[test]
    fn decodes_iso_8859_1() {
        assert_eq!(
            decode_encoded_words("=?ISO-8859-1?Q?c=EDti_a=c7=c3o?="),
            "cíti aÇÃo"
        );
    }

    #[test]
    fn decodes_windows_1252() {
        assert_eq!(
            decode_encoded_words("=?windows-1252?Q?=80_off_20%25?="),
            "€ off 20%25"
        );
    }

    #[test]
    fn malformed_words_left_untouched() {
        assert_eq!(
            decode_encoded_words("=?UTF-8?Q?unterminated"),
            "=?UTF-8?Q?unterminated"
        );
        assert_eq!(decode_encoded_words("plain =?bogus"), "plain =?bogus");
        assert_eq!(
            decode_encoded_words("=?UTF-8?Z?not_base64?="),
            "=?UTF-8?Z?not_base64?="
        );
    }

    #[test]
    fn encodes_underscore_as_space_only_inside_words() {
        assert_eq!(decode_encoded_words("=?UTF-8?Q?a_b?= c_d"), "a b c_d");
    }
}
