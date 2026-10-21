use std::collections::HashSet;

use base64::Engine;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextOp {
    Trim,
    StripBlankLines,
    SortLines,
    SortLinesDesc,
    UniqueLines,
    Upper,
    Lower,
    JoinLines,
    UrlEncode,
    UrlDecode,
    Base64Encode,
    Base64Decode,
}

pub const OPS: [TextOp; 12] = [
    TextOp::Trim,
    TextOp::StripBlankLines,
    TextOp::SortLines,
    TextOp::SortLinesDesc,
    TextOp::UniqueLines,
    TextOp::Upper,
    TextOp::Lower,
    TextOp::JoinLines,
    TextOp::UrlEncode,
    TextOp::UrlDecode,
    TextOp::Base64Encode,
    TextOp::Base64Decode,
];

impl TextOp {
    pub fn label(self) -> &'static str {
        match self {
            Self::Trim => "去掉每行首尾空白",
            Self::StripBlankLines => "删掉空行",
            Self::SortLines => "按行升序排列",
            Self::SortLinesDesc => "按行降序排列",
            Self::UniqueLines => "删掉重复行",
            Self::Upper => "转成大写",
            Self::Lower => "转成小写",
            Self::JoinLines => "合并成一行",
            Self::UrlEncode => "URL 编码",
            Self::UrlDecode => "URL 解码",
            Self::Base64Encode => "Base64 编码",
            Self::Base64Decode => "Base64 解码",
        }
    }
}

pub fn first_line(text: &str) -> String {
    lines(text)
        .first()
        .map(|line| line.trim())
        .unwrap_or("")
        .to_string()
}

pub fn apply(source: &str, op: TextOp) -> String {
    match op {
        TextOp::Trim => lines(source)
            .iter()
            .map(|line| line.trim())
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
        TextOp::StripBlankLines => lines(source)
            .into_iter()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        TextOp::SortLines => {
            let mut out = lines(source);
            out.sort();
            out.join("\n")
        }
        TextOp::SortLinesDesc => {
            let mut out = lines(source);
            out.sort();
            out.reverse();
            out.join("\n")
        }
        TextOp::UniqueLines => {
            let mut seen = HashSet::new();
            lines(source)
                .into_iter()
                .filter(|line| seen.insert(line.to_string()))
                .collect::<Vec<_>>()
                .join("\n")
        }
        TextOp::Upper => source.to_uppercase(),
        TextOp::Lower => source.to_lowercase(),
        TextOp::JoinLines => lines(source)
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        TextOp::UrlEncode => url_encode(source),
        TextOp::UrlDecode => url_decode(source),
        TextOp::Base64Encode => base64::engine::general_purpose::STANDARD.encode(source.as_bytes()),
        TextOp::Base64Decode => {
            let trimmed = source.trim();
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(trimmed)
                .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(trimmed));
            decoded
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .unwrap_or_else(|| source.to_string())
        }
    }
}

fn lines(source: &str) -> Vec<&str> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\n' || bytes[index] == b'\r' {
            out.push(&source[start..index]);
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
            start = index + 1;
        }
        index += 1;
    }
    out.push(&source[start..]);
    out
}

fn url_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::new();
    for byte in value.as_bytes() {
        let byte = *byte;
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0F) as usize] as char);
        }
    }
    out
}

fn url_decode(value: &str) -> String {
    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    let chars: Vec<char> = value.chars().collect();
    let mut bytes = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '%' {
            let hi = chars.get(index + 1).copied().and_then(|c| hex(c as u8));
            let lo = chars.get(index + 2).copied().and_then(|c| hex(c as u8));
            if let (Some(hi), Some(lo)) = (hi, lo) {
                bytes.push((hi << 4) | lo);
                index += 3;
                continue;
            }
            bytes.push(b'%');
        } else if ch == '+' {
            bytes.push(b' ');
        } else {
            let mut buffer = [0u8; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
        }
        index += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{apply, first_line, TextOp};

    #[test]
    fn line_operations_match_the_compose_vectors() {
        let multiline = "  b\na\n\na\n  c ";
        assert_eq!(apply(multiline, TextOp::Trim), "b\na\n\na\nc");
        assert_eq!(apply(multiline, TextOp::StripBlankLines), "  b\na\na\n  c ");
        assert_eq!(apply(multiline, TextOp::JoinLines), "b a a c");
        let unsorted = "banana\napple\nCherry\napple";
        assert_eq!(
            apply(unsorted, TextOp::SortLines),
            "Cherry\napple\napple\nbanana"
        );
        assert_eq!(
            apply(unsorted, TextOp::SortLinesDesc),
            "banana\napple\napple\nCherry"
        );
        assert_eq!(
            apply(unsorted, TextOp::UniqueLines),
            "banana\napple\nCherry"
        );
    }

    #[test]
    fn line_endings_are_split_like_kotlin() {
        assert_eq!(apply("a\r\nb\rc", TextOp::JoinLines), "a b c");
        assert_eq!(apply("a\n", TextOp::UniqueLines), "a\n");
        assert_eq!(first_line("  hello\nworld"), "hello");
    }

    #[test]
    fn case_operations() {
        assert_eq!(apply("abc", TextOp::Upper), "ABC");
        assert_eq!(apply("ABC", TextOp::Lower), "abc");
    }

    #[test]
    fn url_codec_keeps_unreserved_and_round_trips() {
        let plain = "a-b._~Z0 /?&=ü";
        assert_eq!(
            apply(plain, TextOp::UrlEncode),
            "a-b._~Z0%20%2F%3F%26%3D%C3%BC"
        );
        assert_eq!(
            apply(&apply(plain, TextOp::UrlEncode), TextOp::UrlDecode),
            plain
        );
        assert_eq!(apply("a+b", TextOp::UrlDecode), "a b");
        assert_eq!(apply("%zz", TextOp::UrlDecode), "%zz");
    }

    #[test]
    fn base64_codec_round_trips() {
        let plain = "剪贴板 / clipboard";
        assert_eq!(
            apply(plain, TextOp::Base64Encode),
            "5Ymq6LS05p2/IC8gY2xpcGJvYXJk"
        );
        assert_eq!(
            apply(&apply(plain, TextOp::Base64Encode), TextOp::Base64Decode),
            plain
        );
        assert_eq!(apply("!!", TextOp::Base64Decode), "!!");
    }
}
