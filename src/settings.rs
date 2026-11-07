use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Settings {
    width: u32,
}

pub fn path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("WindowScaler")
        .join("settings.json")
}

pub fn load(path: &Path) -> u32 {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
        .map(|s| s.width)
        .filter(|n| *n > 0 && *n <= i32::MAX as u32)
        .unwrap_or(240)
}

pub fn save(path: &Path, width: u32) -> std::io::Result<()> {
    if width == 0 || width > i32::MAX as u32 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "宽度无效",
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, serde_json::to_vec_pretty(&Settings { width })?)?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_settings_fall_back_and_only_width_is_persisted() {
        let dir = std::env::temp_dir().join(format!(
            "window-scaler-settings-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        fs::write(&file, "broken").unwrap();
        assert_eq!(load(&file), 240);
        save(&file, 480).unwrap();
        assert_eq!(load(&file), 480);
        assert!(save(&file, 0).is_err());
        assert_eq!(load(&file), 480);
        let json: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 1);
        fs::remove_file(&file).unwrap();
        fs::remove_dir(&dir).unwrap();
    }
}
