use std::io::Read;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager, WebviewWindow};

const MAX_AVATAR_BYTES: usize = 512 * 1024;
const PNG_PREFIX: &str = "data:image/png;base64,iVBORw0KGgo";

fn valid_avatar(value: &str) -> bool {
    if value.len() > MAX_AVATAR_BYTES || !value.starts_with(PNG_PREFIX) {
        return false;
    }
    let data = &value["data:image/png;base64,".len()..];
    if data.len() % 4 != 0 {
        return false;
    }
    let body = data.trim_end_matches('=');
    data.len() - body.len() <= 2
        && body.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/')
}
fn avatar_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_local_data_dir().map(|path| path.join("island-avatar.txt"))
        .map_err(|_| "AVATAR_PATH_FAILED".into())
}
fn read_avatar(path: &Path) -> Result<Option<String>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("AVATAR_READ_FAILED".into()),
    };
    let mut value = String::new();
    file.take((MAX_AVATAR_BYTES + 1) as u64).read_to_string(&mut value)
        .map_err(|_| "AVATAR_READ_FAILED")?;
    if value.is_empty() { return Ok(None); }
    if valid_avatar(&value) { Ok(Some(value)) } else { Ok(None) }
}
fn save_avatar(path: &Path, avatar: Option<&str>) -> Result<(), String> {
    if avatar.is_some_and(|value| !valid_avatar(value)) { return Err("AVATAR_INVALID".into()); }
    std::fs::create_dir_all(path.parent().ok_or("AVATAR_PATH_FAILED")?)
        .map_err(|_| "AVATAR_SAVE_FAILED")?;
    let pending = path.with_extension("txt.pending");
    std::fs::write(&pending, avatar.unwrap_or_default()).map_err(|_| "AVATAR_SAVE_FAILED")?;
    std::fs::rename(pending, path).map_err(|_| "AVATAR_SAVE_FAILED".into())
}

#[tauri::command]
pub fn get_island_avatar(app: tauri::AppHandle) -> Result<Option<String>, String> {
    read_avatar(&avatar_path(&app)?)
}
#[tauri::command]
pub fn set_island_avatar(app: tauri::AppHandle, window: WebviewWindow, avatar: Option<String>) -> Result<(), String> {
    if window.label() != "settings" { return Err("AVATAR_FORBIDDEN".into()); }
    save_avatar(&avatar_path(&app)?, avatar.as_deref())?;
    let _ = app.emit("island-avatar-changed", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_bounded_png_data() {
        assert!(valid_avatar("data:image/png;base64,iVBORw0KGgo="));
        assert!(!valid_avatar("data:image/svg+xml;base64,iVBORw0KGgo="));
        assert!(!valid_avatar("data:image/png;base64,iVBORw0KGgo<script>"));
        assert!(!valid_avatar(&format!("{}{}", PNG_PREFIX, "A".repeat(MAX_AVATAR_BYTES))));
        assert!(!valid_avatar("data:image/png;base64,iVBORw0KGgo=".replace("=", "==").as_str()));
    }

    #[test]
    fn saving_and_resetting_preserves_only_a_valid_avatar() {
        let path = std::env::temp_dir().join(format!("codex-flow-avatar-test-{}.txt", std::process::id()));
        let value = "data:image/png;base64,iVBORw0KGgo=";
        save_avatar(&path, Some(value)).unwrap();
        assert_eq!(read_avatar(&path).unwrap().as_deref(), Some(value));
        assert!(save_avatar(&path, Some("not an image")).is_err());
        assert_eq!(read_avatar(&path).unwrap().as_deref(), Some(value));
        save_avatar(&path, None).unwrap();
        assert_eq!(read_avatar(&path).unwrap(), None);
        std::fs::remove_file(path).unwrap();
    }
}
