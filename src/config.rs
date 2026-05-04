use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Rgb,
    Temp,
}

#[derive(Deserialize)]
struct ModeFile {
    mode: String,
}

/// モードファイルを読み込む。ファイルが存在しなければ Ok(None)。
pub fn read_mode(path: &Path) -> Result<Option<Mode>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read mode file: {}", path.display()))?;
    let parsed: ModeFile = toml::from_str(&content)
        .with_context(|| format!("failed to parse mode file: {}", path.display()))?;
    match parsed.mode.as_str() {
        "rgb" => Ok(Some(Mode::Rgb)),
        "temp" => Ok(Some(Mode::Temp)),
        other => Err(anyhow!(
            "invalid mode value '{}': expected 'rgb' or 'temp'",
            other
        )),
    }
}

/// モードファイルを書き出す。親ディレクトリが存在する前提。
pub fn write_mode(path: &Path, mode: Mode) -> Result<()> {
    let m = match mode {
        Mode::Rgb => "rgb",
        Mode::Temp => "temp",
    };
    let content = format!("mode = \"{}\"\n", m);
    fs::write(path, content)
        .with_context(|| format!("failed to write mode file: {}", path.display()))?;
    Ok(())
}

#[derive(Deserialize, Debug, Clone)]
pub struct DefaultDevice {
    pub id: String,
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Deserialize)]
struct DevicesFile {
    default: Option<DefaultDevice>,
}

/// devices ファイルを読み、[default] セクションを返す。
/// 存在しないか [default] が無いか id が空ならエラー。
pub fn load_devices(path: &Path) -> Result<DefaultDevice> {
    if !path.exists() {
        return Err(anyhow!("devices file not found: {}", path.display()));
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read devices file: {}", path.display()))?;
    let parsed: DevicesFile = toml::from_str(&content)
        .with_context(|| format!("failed to parse devices file: {}", path.display()))?;
    let device = parsed
        .default
        .ok_or_else(|| anyhow!("[default] section not found in {}", path.display()))?;
    if device.id.is_empty() {
        return Err(anyhow!("[default] id is empty in {}", path.display()));
    }
    Ok(device)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn round_trip_rgb() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mode");
        write_mode(&path, Mode::Rgb).unwrap();
        assert_eq!(read_mode(&path).unwrap(), Some(Mode::Rgb));
    }

    #[test]
    fn round_trip_temp() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mode");
        write_mode(&path, Mode::Temp).unwrap();
        assert_eq!(read_mode(&path).unwrap(), Some(Mode::Temp));
    }

    #[test]
    fn missing_file_returns_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mode");
        assert_eq!(read_mode(&path).unwrap(), None);
    }

    #[test]
    fn invalid_value_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mode");
        fs::write(&path, "mode = \"unknown\"").unwrap();
        assert!(read_mode(&path).is_err());
    }

    #[test]
    fn malformed_toml_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("mode");
        fs::write(&path, "this is not toml ===").unwrap();
        assert!(read_mode(&path).is_err());
    }

    #[test]
    fn load_devices_with_default_section_ok() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        fs::write(
            &path,
            r#"
[default]
id = "01-202311241234-12345678"
type = "Color Bulb"
name = "Living Bulb"
"#,
        )
        .unwrap();
        let device = load_devices(&path).unwrap();
        assert_eq!(device.id, "01-202311241234-12345678");
        assert_eq!(device.r#type, "Color Bulb");
        assert_eq!(device.name, "Living Bulb");
    }

    #[test]
    fn load_devices_missing_file_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        assert!(load_devices(&path).is_err());
    }

    #[test]
    fn load_devices_missing_default_section_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        fs::write(
            &path,
            r#"[other]
id = "x"
"#,
        )
        .unwrap();
        assert!(load_devices(&path).is_err());
    }

    #[test]
    fn load_devices_empty_id_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        fs::write(
            &path,
            r#"[default]
id = ""
type = "Color Bulb"
"#,
        )
        .unwrap();
        assert!(load_devices(&path).is_err());
    }

    #[test]
    fn load_devices_id_only_minimum_ok() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        fs::write(
            &path,
            r#"[default]
id = "abc"
"#,
        )
        .unwrap();
        let device = load_devices(&path).unwrap();
        assert_eq!(device.id, "abc");
        assert_eq!(device.r#type, "");
    }
}
