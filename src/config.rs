use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Context as AnyhowContext;
use anyhow::{anyhow, Result};
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
        .map_err(|e| anyhow!("failed to parse mode file {}: {}", path.display(), e))?;
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

/// .env 形式テキストを KEY=VALUE のマップにパースする。
/// 空行と '#' で始まるコメント行は無視する。値の前後空白は trim する。
/// 値の引用符 (' or ") は剥がさない (op inject の出力もリテラルもそのまま扱える)。
pub fn parse_env_content(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    map
}

/// 値のいずれかが op:// で始まれば true。
pub fn has_op_reference(env: &HashMap<String, String>) -> bool {
    env.values().any(|v| v.starts_with("op://"))
}

#[derive(Debug, Clone)]
pub struct Credentials {
    pub token: String,
    pub secret: String,
}

/// .env を読み、必要なら op inject で解決し、SWITCHBOT_TOKEN / SWITCHBOT_SECRET を返す。
pub fn load_credentials(env_path: &Path) -> Result<Credentials> {
    if !env_path.exists() {
        return Err(anyhow!(".env file not found: {}", env_path.display()));
    }
    let raw = fs::read_to_string(env_path)
        .with_context(|| format!("failed to read .env: {}", env_path.display()))?;
    let raw_map = parse_env_content(&raw);

    let resolved = if has_op_reference(&raw_map) {
        resolve_with_op_inject(env_path)?
    } else {
        raw_map
    };

    let token = resolved
        .get("SWITCHBOT_TOKEN")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "SWITCHBOT_TOKEN is empty or missing in {}",
                env_path.display()
            )
        })?
        .clone();
    let secret = resolved
        .get("SWITCHBOT_SECRET")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "SWITCHBOT_SECRET is empty or missing in {}",
                env_path.display()
            )
        })?
        .clone();

    Ok(Credentials { token, secret })
}

fn resolve_with_op_inject(env_path: &Path) -> Result<HashMap<String, String>> {
    let output = Command::new("op")
        .arg("inject")
        .arg("-i")
        .arg(env_path)
        .output()
        .map_err(|e| {
            anyhow!(
                "failed to execute `op inject`: {}. Is the 1Password CLI (`op`) installed and on PATH?",
                e
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "`op inject` failed (1Password unlock 状態を確認してください): {}",
            stderr.trim()
        ));
    }
    let stdout =
        String::from_utf8(output.stdout).context("`op inject` returned non-UTF8 output")?;
    Ok(parse_env_content(&stdout))
}

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[derive(Debug)]
pub struct Context {
    pub credentials: Credentials,
    pub device: DefaultDevice,
    pub mode_path: PathBuf,
    pub log_path: PathBuf,
}

const ENV_TEMPLATE: &str = "\
# 1Password 連携 (推奨):
SWITCHBOT_TOKEN=op://Personal/SwitchBot/token
SWITCHBOT_SECRET=op://Personal/SwitchBot/secret
# 直接値を書く場合 (テスト用途等):
# SWITCHBOT_TOKEN=...
# SWITCHBOT_SECRET=...
";

const DEVICES_TEMPLATE: &str = "\
# ~/.switchbot/devices
# `switchbot list` の出力をリダイレクトするか、手書きで埋めてください。
[default]
id = \"\"
type = \"Color Bulb\"
";

/// `~/.switchbot/` ディレクトリを返す。HOME 未設定ならエラー。
pub fn config_dir() -> Result<PathBuf> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| anyhow!("ホームディレクトリを特定できません"))?;
    Ok(base.home_dir().join(".switchbot"))
}

/// 必要なディレクトリ・テンプレートを用意し、Context を組み立てる。
/// `.env` または `devices` がなければテンプレを書き出して `BootstrapNeeded` 相当のエラーで返す。
pub fn load_context() -> Result<Context> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;

    let env_path = dir.join(".env");
    let devices_path = dir.join("devices");
    let mode_path = dir.join("mode");
    let log_path = dir.join("log");

    let mut needs_setup = Vec::new();

    if !env_path.exists() {
        fs::write(&env_path, ENV_TEMPLATE)
            .with_context(|| format!("failed to write template: {}", env_path.display()))?;
        fs::set_permissions(&env_path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to chmod 600: {}", env_path.display()))?;
        needs_setup.push(format!("{} を編集してください", env_path.display()));
    }

    if !devices_path.exists() {
        fs::write(&devices_path, DEVICES_TEMPLATE)
            .with_context(|| format!("failed to write template: {}", devices_path.display()))?;
        needs_setup.push(format!(
            "{} を編集してください (switchbot list で deviceId を確認)",
            devices_path.display()
        ));
    }

    if !needs_setup.is_empty() {
        return Err(anyhow!("{}", needs_setup.join("\n")));
    }

    let credentials = load_credentials(&env_path)?;
    let device = load_devices(&devices_path)?;

    Ok(Context {
        credentials,
        device,
        mode_path,
        log_path,
    })
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

    #[test]
    fn parse_env_basic() {
        let content = "FOO=bar\nBAZ=qux\n";
        let map = parse_env_content(content);
        assert_eq!(map.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(map.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    fn parse_env_skips_comments_and_blank() {
        let content = "\n# comment\nFOO=bar\n\n# another\nBAZ=qux\n";
        let map = parse_env_content(content);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn parse_env_trims_whitespace() {
        let content = "  FOO  =  bar  \n";
        let map = parse_env_content(content);
        assert_eq!(map.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn parse_env_keeps_op_reference_literally() {
        let content = "TOKEN=op://Personal/Item/credential\n";
        let map = parse_env_content(content);
        assert_eq!(
            map.get("TOKEN"),
            Some(&"op://Personal/Item/credential".to_string())
        );
    }

    #[test]
    fn has_op_reference_detects_op_prefix() {
        let mut map = HashMap::new();
        map.insert("A".to_string(), "plain".to_string());
        assert!(!has_op_reference(&map));
        map.insert("B".to_string(), "op://x/y/z".to_string());
        assert!(has_op_reference(&map));
    }

    #[test]
    fn has_op_reference_empty_map_false() {
        let map: HashMap<String, String> = HashMap::new();
        assert!(!has_op_reference(&map));
    }

    #[test]
    fn load_credentials_plain_values_ok() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "SWITCHBOT_TOKEN=tok\nSWITCHBOT_SECRET=sec\n").unwrap();
        let creds = load_credentials(&path).unwrap();
        assert_eq!(creds.token, "tok");
        assert_eq!(creds.secret, "sec");
    }

    #[test]
    fn load_credentials_missing_file_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".env");
        assert!(load_credentials(&path).is_err());
    }

    #[test]
    fn load_credentials_missing_token_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "SWITCHBOT_SECRET=sec\n").unwrap();
        assert!(load_credentials(&path).is_err());
    }

    #[test]
    fn load_credentials_empty_token_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "SWITCHBOT_TOKEN=\nSWITCHBOT_SECRET=sec\n").unwrap();
        assert!(load_credentials(&path).is_err());
    }

    #[test]
    fn env_template_contains_required_keys() {
        assert!(ENV_TEMPLATE.contains("SWITCHBOT_TOKEN="));
        assert!(ENV_TEMPLATE.contains("SWITCHBOT_SECRET="));
        assert!(ENV_TEMPLATE.contains("op://"));
    }

    #[test]
    fn devices_template_contains_default_section() {
        assert!(DEVICES_TEMPLATE.contains("[default]"));
        assert!(DEVICES_TEMPLATE.contains("id = \"\""));
    }
}
