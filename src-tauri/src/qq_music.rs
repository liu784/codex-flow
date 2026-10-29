//! Shows only the current QQ Music lyric, sourced from its local cache.
use crate::qrc::{self, Line, Word};
use serde::Serialize;
use std::{path::{Path, PathBuf}, time::{Duration, Instant}};
use tauri::{Emitter, Manager};

#[derive(Default)]
struct Snapshot { title: String, artist: String, position_ms: u64, playing: bool }

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LyricFrame {
    line_start_ms: u64,
    position_ms: u64,
    playing: bool,
    words: Vec<Word>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cover: Option<String>,
}

#[derive(Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct MediaFrame {
    #[serde(skip_serializing_if = "Option::is_none")]
    cover: Option<String>,
    playing: bool,
}

#[cfg(windows)]
unsafe extern "C" {
    fn cf_qq_music_snapshot(callback: extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_char, *const std::ffi::c_char, i64, i32), context: *mut std::ffi::c_void) -> i32;
    fn cf_qq_music_cover(callback: extern "C" fn(*mut std::ffi::c_void, *const u8, usize), context: *mut std::ffi::c_void) -> i32;
    fn cf_process_running(name: *const u16) -> i32;
}

#[cfg(windows)]
fn process_running(name: &str) -> Option<bool> {
    if name.is_empty() || name.contains('\0') { return None; }
    let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    match unsafe { cf_process_running(name.as_ptr()) } { 0 => Some(false), 1 => Some(true), _ => None }
}

#[cfg(not(windows))]
fn process_running(_: &str) -> Option<bool> { None }

#[cfg(windows)]
extern "C" fn collect_snapshot(context: *mut std::ffi::c_void, title: *const std::ffi::c_char, artist: *const std::ffi::c_char, position: i64, playing: i32) {
    if context.is_null() || title.is_null() || artist.is_null() { return; }
    let snapshot = unsafe { &mut *context.cast::<Snapshot>() };
    snapshot.title = unsafe { std::ffi::CStr::from_ptr(title) }.to_string_lossy().into_owned();
    snapshot.artist = unsafe { std::ffi::CStr::from_ptr(artist) }.to_string_lossy().into_owned();
    snapshot.position_ms = position.max(0) as u64;
    snapshot.playing = playing == 1;
}

#[cfg(windows)]
extern "C" fn collect_cover(context: *mut std::ffi::c_void, data: *const u8, len: usize) {
    if context.is_null() || data.is_null() || len < 16 || len > 2 * 1024 * 1024 { return; }
    let out = unsafe { &mut *context.cast::<Vec<u8>>() };
    out.clear();
    out.extend_from_slice(unsafe { std::slice::from_raw_parts(data, len) });
}

#[cfg(windows)]
fn snapshot() -> Option<Snapshot> {
    let mut result = Snapshot::default();
    let status = unsafe { cf_qq_music_snapshot(collect_snapshot, (&mut result as *mut Snapshot).cast()) };
    (status == 1).then_some(result)
}

#[cfg(not(windows))]
fn snapshot() -> Option<Snapshot> { None }

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn cover_data_url(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 16 || bytes.len() > 2 * 1024 * 1024 {
        return None;
    }
    let mime = if bytes.starts_with(b"\xFF\xD8\xFF") {
        "image/jpeg"
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else {
        return None;
    };
    Some(format!("data:{mime};base64,{}", base64_encode(bytes)))
}

fn picture_cache_dir() -> Option<PathBuf> {
    Some(cache_dir()?.parent()?.join("QQMusicPicture"))
}

fn load_cached_picture_fallback() -> Option<String> {
    let dir = picture_cache_dir()?;
    let mut newest_album: Option<(std::time::SystemTime, PathBuf)> = None;
    let mut newest_any: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.take(5_000).flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() || meta.len() < 256 || meta.len() > 2 * 1024 * 1024 {
            continue;
        }
        let Ok(modified) = meta.modified() else {
            continue;
        };
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let is_small_grid = name_str.contains("R150x150") || name_str.contains("R120x120");
        if !is_small_grid && newest_album.as_ref().is_none_or(|(prev, _)| modified > *prev) {
            newest_album = Some((modified, path.clone()));
        }
        if newest_any.as_ref().is_none_or(|(prev, _)| modified > *prev) {
            newest_any = Some((modified, path));
        }
    }
    let chosen = match (newest_album, newest_any) {
        (Some((album_t, album_p)), Some((any_t, any_p))) => {
            if any_t.duration_since(album_t).unwrap_or_default() > Duration::from_secs(10) {
                any_p
            } else {
                album_p
            }
        }
        (Some((_, p)), None) | (None, Some((_, p))) => p,
        (None, None) => return None,
    };
    let bytes = std::fs::read(chosen).ok()?;
    cover_data_url(&bytes)
}

#[cfg(windows)]
fn load_gsmtc_cover() -> Option<String> {
    let mut raw = Vec::new();
    let status = unsafe { cf_qq_music_cover(collect_cover, (&mut raw as *mut Vec<u8>).cast()) };
    if status == 1 {
        cover_data_url(&raw)
    } else {
        None
    }
}

#[cfg(not(windows))]
fn load_gsmtc_cover() -> Option<String> {
    None
}

#[cfg(test)]
fn load_cover() -> Option<String> {
    load_gsmtc_cover().or_else(load_cached_picture_fallback)
}

fn normalized(value: &str) -> String {
    value.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn cache_dir() -> Option<PathBuf> {
    let config = PathBuf::from(std::env::var_os("APPDATA")?).join("Tencent/QQMusic/WebkitCachePath.ini");
    if config.metadata().ok()?.len() > 32 * 1024 { return None; }
    let ini = std::fs::read_to_string(config).ok()?;
    let path = ini.lines().filter_map(|line| line.trim().split_once('='))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("Path"))?.1.trim();
    Some(Path::new(path).parent()?.join("QQMusicLyricNew"))
}

fn find_song_file(dir: &Path, title: &str, artist: &str) -> Option<PathBuf> {
    let title = normalized(title);
    let artist = normalized(artist);
    if title.is_empty() || artist.is_empty() { return None; }
    let mut best: Option<(u16, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.take(20_000).flatten() {
        if !entry.file_type().ok().is_some_and(|kind| kind.is_file()) { continue; }
        let name = entry.file_name();
        let Some(name) = name.to_str().and_then(|name| name.strip_suffix("_qm.qrc")) else { continue };
        let mut parts = name.splitn(4, " - ");
        let (Some(file_artist), Some(file_title)) = (parts.next(), parts.next()) else { continue };
        let file_title = normalized(file_title);
        let file_artist = normalized(file_artist);
        if file_title.is_empty() || file_artist.is_empty() { continue; }
        let title_score = if file_title == title { 100 } else if file_title.contains(&title) || title.contains(&file_title) { 60 } else { continue };
        let artist_score = if file_artist == artist { 50 } else if file_artist.contains(&artist) || artist.contains(&file_artist) { 30 } else { continue };
        let score = title_score + artist_score;
        if score < 110 || best.as_ref().is_some_and(|(previous, _)| *previous >= score) { continue; }
        best = Some((score, entry.path()));
    }
    best.map(|(_, path)| path)
}

fn load_current(title: &str, artist: &str) -> Option<Vec<Line>> {
    let path = find_song_file(&cache_dir()?, title, artist)?;
    if path.metadata().ok()?.len() > 512 * 1024 { return None; }
    qrc::decode(&std::fs::read(path).ok()?)
}

fn current_line(lines: &[Line], position_ms: u64) -> Option<&Line> {
    let line = lines.iter().rev().find(|line| line.start_ms <= position_ms)?;
    (position_ms <= line.start_ms.saturating_add(line.duration_ms).saturating_add(2000)).then_some(line)
}

pub(crate) fn setup(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut current = (String::new(), String::new());
        let mut lines = None;
        let mut cover: Option<String> = None;
        let mut prev_cover: Option<String> = None;
        let mut cover_is_fallback = false;
        let mut track_changed_at = Instant::now();
        let mut next_lookup = Instant::now();
        let mut next_cover_lookup = Instant::now();
        let mut next_presence = Instant::now();
        let mut next_media_emit = Instant::now();
        let mut last_media: Option<MediaFrame> = None;
        let mut visible = false;
        let mut media_visible = false;
        loop {
            if app.state::<crate::desktop::DesktopState>().is_quitting() { break; }
            if Instant::now() >= next_presence {
                // Repeated delivery also covers the frontend listener attaching after startup.
                if let Some(open) = process_running("QQMusic.exe") {
                    let _ = app.emit_to("main", "qq-music-open", open);
                }
                next_presence = Instant::now() + Duration::from_secs(1);
            }
            if let Some(snapshot) = snapshot() {
                let key = (snapshot.title.clone(), snapshot.artist.clone());
                if current != key {
                    current = key;
                    lines = None;
                    if cover.is_some() {
                        prev_cover = cover.take();
                    }
                    cover_is_fallback = false;
                    track_changed_at = Instant::now();
                    next_lookup = Instant::now();
                    next_cover_lookup = Instant::now();
                }
                if lines.is_none() && Instant::now() >= next_lookup {
                    lines = load_current(&snapshot.title, &snapshot.artist);
                    next_lookup = Instant::now() + Duration::from_secs(2);
                }
                let should_check_cover = cover.is_none()
                    || cover_is_fallback
                    || (track_changed_at.elapsed() < Duration::from_secs(5)
                        && prev_cover.is_some()
                        && cover == prev_cover);
                if should_check_cover && Instant::now() >= next_cover_lookup {
                    if let Some(gsmtc_url) = load_gsmtc_cover() {
                        let same_as_prev = prev_cover.as_ref() == Some(&gsmtc_url);
                        cover = Some(gsmtc_url);
                        cover_is_fallback = false;
                        next_cover_lookup = Instant::now()
                            + if same_as_prev && track_changed_at.elapsed() < Duration::from_secs(5)
                            {
                                Duration::from_millis(500)
                            } else {
                                Duration::from_secs(2)
                            };
                    } else {
                        if cover.is_none() && track_changed_at.elapsed() >= Duration::from_millis(500) {
                            if let Some(fallback_url) = load_cached_picture_fallback() {
                                cover = Some(fallback_url);
                                cover_is_fallback = true;
                            }
                        }
                        next_cover_lookup = Instant::now()
                            + if track_changed_at.elapsed() < Duration::from_secs(3) {
                                Duration::from_millis(500)
                            } else {
                                Duration::from_secs(2)
                            };
                    }
                }
                let media = MediaFrame {
                    cover: cover.clone(),
                    playing: snapshot.playing,
                };
                if last_media.as_ref() != Some(&media) || Instant::now() >= next_media_emit {
                    let _ = app.emit_to("main", "qq-music-media", Some(media.clone()));
                    last_media = Some(media);
                    next_media_emit = Instant::now() + Duration::from_secs(2);
                }
                media_visible = true;
                let frame = lines.as_deref().and_then(|lines| current_line(lines, snapshot.position_ms))
                    .map(|line| LyricFrame {
                        line_start_ms: line.start_ms,
                        position_ms: snapshot.position_ms,
                        playing: snapshot.playing,
                        words: line.words.clone(),
                        cover: cover.clone(),
                    });
                if let Some(frame) = frame {
                    let _ = app.emit_to("main", "island-lyric", Some(frame));
                    visible = true;
                } else if visible {
                    let _ = app.emit_to("main", "island-lyric", Option::<LyricFrame>::None);
                    visible = false;
                }
            } else {
                current = (String::new(), String::new());
                lines = None;
                cover = None;
                prev_cover = None;
                cover_is_fallback = false;
                last_media = None;
                if visible {
                    let _ = app.emit_to("main", "island-lyric", Option::<LyricFrame>::None);
                    visible = false;
                }
                if media_visible {
                    let _ = app.emit_to("main", "qq-music-media", Option::<MediaFrame>::None);
                    media_visible = false;
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn process_presence_detects_real_running_and_missing_executables() {
        let current = std::env::current_exe().unwrap();
        let name = current.file_name().unwrap().to_str().unwrap();
        assert_eq!(process_running(name), Some(true));
        assert_eq!(process_running("codex-flow-process-that-does-not-exist-31ea539c.exe"), Some(false));
        assert_eq!(process_running("bad\0name.exe"), None);
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires QQ Music to be running in the current Windows session"]
    fn live_qq_music_process_is_detected() {
        assert_eq!(process_running("QQMusic.exe"), Some(true));
    }
    #[test]
    fn lyric_selection_handles_seek_and_expired_gaps() {
        let lines = vec![Line { start_ms: 1000, duration_ms: 1000, words: vec![] }, Line { start_ms: 6000, duration_ms: 1000, words: vec![] }];
        assert!(current_line(&lines, 500).is_none());
        assert_eq!(current_line(&lines, 1500).unwrap().start_ms, 1000);
        assert!(current_line(&lines, 4500).is_none());
        assert_eq!(current_line(&lines, 6200).unwrap().start_ms, 6000);
        assert_eq!(current_line(&lines, 1200).unwrap().start_ms, 1000);
    }
    #[test]
    fn cover_data_url_encodes_valid_images_and_rejects_unknown_formats() {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpeg.extend_from_slice(&[0u8; 20]);
        let url = cover_data_url(&jpeg).expect("valid jpeg header should encode");
        assert!(url.starts_with("data:image/jpeg;base64,/9j/4"));

        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&[0u8; 16]);
        let png_url = cover_data_url(&png).expect("valid png header should encode");
        assert!(png_url.starts_with("data:image/png;base64,iVBORw0KGgo"));

        assert!(cover_data_url(b"short").is_none());
        assert!(cover_data_url(&[0u8; 32]).is_none());
    }
    #[test]
    #[ignore = "requires an actively playing QQ Music desktop session"]
    fn live_qq_music_cover_extraction() {
        let cover = load_cover().expect("expected album cover from live QQ Music session");
        assert!(
            cover.starts_with("data:image/jpeg;base64,")
                || cover.starts_with("data:image/png;base64,")
                || cover.starts_with("data:image/webp;base64,"),
            "unexpected cover prefix"
        );
        println!("QQ Music cover extracted successfully ({} chars)", cover.len());
    }
    #[test]
    #[ignore = "requires an actively playing QQ Music desktop session"]
    fn live_qq_music_lyric_cache() {
        let music = snapshot().expect("QQ Music session unavailable");
        let lines = load_current(&music.title, &music.artist).expect("current song has no usable local timed lyrics");
        assert!(!lines.is_empty());
        assert!(current_line(&lines, music.position_ms).is_some(), "no lyric matched the current playback time");
        println!("QQ Music playback detected; current lyric and local timing matched; line count: {}; currently playing: {}", lines.len(), music.playing);
    }
    #[test]
    #[ignore = "requires an actively playing QQ Music desktop session"]
    fn live_timing_diagnostic() {
        let first = snapshot().expect("QQ Music session unavailable");
        assert!(first.playing, "QQ Music is not playing");
        let lines = load_current(&first.title, &first.artist).expect("current lyrics unavailable");
        let first_word_offset = current_line(&lines, first.position_ms)
            .and_then(|line| line.words.first().map(|word| word.start_ms as i64 - line.start_ms as i64));
        let started = Instant::now();
        let mut last = first.position_ms;
        let mut errors = Vec::new();
        let mut backwards = 0;
        for _ in 0..25 {
            std::thread::sleep(Duration::from_millis(200));
            let Some(now) = snapshot() else { break };
            if !now.playing || now.title != first.title || now.artist != first.artist { break; }
            if now.position_ms + 50 < last { backwards += 1; }
            let elapsed = started.elapsed().as_millis() as i64;
            errors.push(now.position_ms as i64 - first.position_ms as i64 - elapsed);
            last = now.position_ms;
        }
        assert!(errors.len() >= 5, "not enough live playback samples");
        println!("samples={} error_min_ms={} error_max_ms={} backwards={} first_word_offset_ms={:?}",
            errors.len(), errors.iter().min().unwrap(), errors.iter().max().unwrap(), backwards, first_word_offset);
    }
    #[test]
    #[ignore = "requires an actively playing QQ Music desktop session"]
    fn live_lyric_pace() {
        let music = snapshot().expect("QQ Music session unavailable");
        let lines = load_current(&music.title, &music.artist).expect("current lyrics unavailable");
        let line = current_line(&lines, music.position_ms).expect("current lyric line unavailable");
        let mut durations: Vec<u64> = line.words.iter().filter_map(|word| {
            let count = word.text.chars().filter(|ch| ch.is_alphanumeric()).count() as u64;
            (count > 0).then(|| word.duration_ms / count)
        }).filter(|duration| *duration >= 30).collect();
        durations.sort_unstable();
        assert!(!durations.is_empty());
        println!("Current lyric line timing median: {} ms per timed character", durations[durations.len() / 2]);
    }
}
