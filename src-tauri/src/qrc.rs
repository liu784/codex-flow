//! A bounded decoder for QQ Music's local timed-lyric cache.
//! Cache format and keys are based on QQMusicMonitor (MIT); see notices.
use flate2::read::ZlibDecoder;
use quick_xml::{events::Event, Reader};
use serde::Serialize;
use std::io::Read;

const MAX_FILE: usize = 512 * 1024;
const MAX_DECOMPRESSED: u64 = 1024 * 1024;
const MAGIC: [u8; 11] = [0x98, 0x25, 0xb0, 0xac, 0xe3, 0x02, 0x83, 0x68, 0xe8, 0xfc, 0x6c];
const MASK: [u8; 128] = [
    0xc3,0x4a,0xd6,0xca,0x90,0x67,0xf7,0x52,0xd8,0xa1,0x66,0x62,0x9f,0x5b,0x09,0x00,
    0xc3,0x5e,0x95,0x23,0x9f,0x13,0x11,0x7e,0xd8,0x92,0x3f,0xbc,0x90,0xbb,0x74,0x0e,
    0xc3,0x47,0x74,0x3d,0x90,0xaa,0x3f,0x51,0xd8,0xf4,0x11,0x84,0x9f,0xde,0x95,0x1d,
    0xc3,0xc6,0x09,0xd5,0x9f,0xfa,0x66,0xf9,0xd8,0xf0,0xf7,0xa0,0x90,0xa1,0xd6,0xf3,
    0xc3,0xf3,0xd6,0xa1,0x90,0xa0,0xf7,0xf0,0xd8,0xf9,0x66,0xfa,0x9f,0xd5,0x09,0xc6,
    0xc3,0x1d,0x95,0xde,0x9f,0x84,0x11,0xf4,0xd8,0x51,0x3f,0xaa,0x90,0x3d,0x74,0x47,
    0xc3,0x0e,0x74,0xbb,0x90,0xbc,0x3f,0x92,0xd8,0x7e,0x11,0x13,0x9f,0x23,0x95,0x5e,
    0xc3,0x00,0x09,0x5b,0x9f,0x62,0x66,0xa1,0xd8,0x52,0xf7,0x67,0x90,0xca,0xd6,0x4a,
];

#[cfg(windows)]
unsafe extern "C" { fn cf_qrc_transform(bytes: *mut u8, len: usize, decode: i32) -> i32; }

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Word { pub text: String, pub start_ms: u64, pub duration_ms: u64 }

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Line { pub start_ms: u64, pub duration_ms: u64, pub words: Vec<Word> }

fn mask_at(i: usize) -> u8 { MASK[if i <= 0x7fff { i & 0x7f } else { (i % 0x7fff) & 0x7f }] }

#[cfg(windows)]
pub(crate) fn decode(bytes: &[u8]) -> Option<Vec<Line>> {
    if bytes.len() <= MAGIC.len() || bytes.len() > MAX_FILE || !bytes.starts_with(&MAGIC) { return None; }
    let mut encrypted: Vec<u8> = bytes[MAGIC.len()..].iter().enumerate()
        .map(|(i, byte)| byte ^ mask_at(i + MAGIC.len())).collect();
    if encrypted.len() % 8 != 0 || unsafe { cf_qrc_transform(encrypted.as_mut_ptr(), encrypted.len(), 1) } != 1 { return None; }
    let mut plain = String::new();
    ZlibDecoder::new(encrypted.as_slice()).take(MAX_DECOMPRESSED + 1).read_to_string(&mut plain).ok()?;
    if plain.len() as u64 > MAX_DECOMPRESSED { return None; }
    let contents = lyric_content(&plain)?;
    let lines = parse_text(&contents);
    if lines.is_empty() { None } else { Some(lines) }
}

#[cfg(not(windows))]
pub(crate) fn decode(_bytes: &[u8]) -> Option<Vec<Line>> { None }

fn lyric_content(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(element) | Event::Empty(element)) => {
                for attr in element.attributes().flatten() {
                    if attr.key.as_ref() == "LyricContent" {
                        if let Ok(value) = quick_xml::escape::unescape(attr.value.as_ref()) {
                            if value.contains('[') { return Some(value.into_owned()); }
                        }
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    // Some QQ Music caches contain unescaped quotes inside the attribute.
    let rest = xml.split_once("LyricContent=\"")?.1;
    let end = rest.find("\"/>").or_else(|| rest.find("\"></"))?;
    let raw = &rest[..end];
    Some(quick_xml::escape::unescape(raw).map(|s| s.into_owned()).unwrap_or_else(|_| raw.to_owned()))
}

fn time_pair(value: &str) -> Option<(u64, u64)> {
    let (start, duration) = value.split_once(',')?;
    let start: u64 = start.trim().parse().ok()?;
    let duration: u64 = duration.trim().parse().ok()?;
    if start > 86_400_000 || duration > 3_600_000 { return None; }
    Some((start, duration))
}

pub(crate) fn parse_text(text: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for raw in text.lines().take(2000) {
        let Some(raw) = raw.trim().strip_prefix('[') else { continue };
        let Some((header, body)) = raw.split_once(']') else { continue };
        let Some((start_ms, duration_ms)) = time_pair(header) else { continue };
        let mut words = Vec::new();
        let mut pending = String::new();
        let mut rest = body;
        while !rest.is_empty() && words.len() < 200 {
            let Some(open) = rest.find('(') else { pending.push_str(rest); break };
            pending.push_str(&rest[..open]);
            rest = &rest[open + 1..];
            if let Some(close) = rest.find(')') {
                if let Some((start, duration)) = time_pair(&rest[..close]) {
                    if !pending.is_empty() {
                        words.push(Word { text: std::mem::take(&mut pending), start_ms: start, duration_ms: duration.max(1) });
                    }
                    rest = &rest[close + 1..];
                    continue;
                }
            }
            pending.push('(');
        }
        if !pending.is_empty() && !words.is_empty() {
            words.last_mut().unwrap().text.push_str(&pending);
        }
        if !words.is_empty() && words.iter().map(|word| word.text.chars().count()).sum::<usize>() <= 500 {
            lines.push(Line { start_ms, duration_ms, words });
        }
    }
    lines.sort_by_key(|line| line.start_ms);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timed_words_and_literal_parentheses_are_preserved() {
        let lines = parse_text("[1000,2000]你(1000,300)好(1300,400)(live)(1700,500)\n[ar:metadata]");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].words.iter().map(|w| w.text.as_str()).collect::<String>(), "你好(live)");
        assert_eq!(lines[0].words[1].start_ms, 1300);
    }
    #[test]
    fn xml_entities_and_malformed_timestamps_are_safe() {
        let xml = "<root><Lyric_1 LyricContent=\"[100,500]A&amp;B(100,100)\"/></root>";
        assert_eq!(parse_text(&lyric_content(xml).unwrap())[0].words[0].text, "A&B");
        assert!(parse_text("[bad,99]oops(1,2)\n[9999999999,3]bad(1,2)").is_empty());
    }
    #[cfg(windows)]
    #[test]
    fn generated_cache_roundtrips_and_rejects_invalid_blocks() {
        use flate2::{write::ZlibEncoder, Compression};
        use std::io::Write;
        let mut zip = ZlibEncoder::new(Vec::new(), Compression::fast());
        zip.write_all(b"<root><Lyric_1 LyricContent=\"[1000,2000]Hello(1000,2000)\"/></root>").unwrap();
        let mut encrypted = zip.finish().unwrap();
        encrypted.resize((encrypted.len() + 7) / 8 * 8, 0);
        assert_eq!(unsafe { cf_qrc_transform(encrypted.as_mut_ptr(), encrypted.len(), 0) }, 1);
        let mut cache = MAGIC.to_vec();
        cache.extend(encrypted.iter().enumerate().map(|(i, byte)| byte ^ mask_at(i + MAGIC.len())));
        assert_eq!(decode(&cache).unwrap()[0].words[0].text, "Hello");
        cache.pop();
        assert!(decode(&cache).is_none());
    }
}
