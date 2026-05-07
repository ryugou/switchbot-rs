use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context as AnyhowContext;
use anyhow::{anyhow, Result};
use serde::Deserialize;

/// `load_context()` の失敗種別。setup 系は stderr のみ、runtime 系は notify + log まで流す。
#[derive(Debug)]
pub enum LoadContextError {
    /// 初回 bootstrap (テンプレ書き出し直後) や HOME 不在など、ユーザーが対話的にセットアップ中の状態。
    /// stderr のみで通知・ログには載せない。
    Setup(anyhow::Error),
    /// 1Password 解決失敗、credentials 取得失敗など、ユーザー外部要因の runtime エラー。
    /// stderr + 通知 + ログに流す。
    Runtime(anyhow::Error),
}

impl LoadContextError {
    pub fn should_notify(&self) -> bool {
        matches!(self, Self::Runtime(_))
    }
}

impl std::fmt::Display for LoadContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Setup(e) | Self::Runtime(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for LoadContextError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Rgb,
    Temp,
}

impl Mode {
    /// Mode の公開ラベル ("rgb" / "temp")。CLI 出力・JSON・モードファイル全てで使う。
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Rgb => "rgb",
            Mode::Temp => "temp",
        }
    }
}

#[derive(Deserialize)]
struct ModeFile {
    mode: String,
}

/// モードファイルを読み込む。ファイルが存在しなければ Ok(None)。
pub fn read_mode(path: &Path) -> Result<Option<Mode>> {
    let content = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e).with_context(|| format!("failed to read mode file: {}", path.display()))
        }
    };
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

/// モードファイルを atomic write で書き出す。親ディレクトリが存在する前提。
/// 同時実行時の tmp 衝突を避けるため NamedTempFile を使い、persist で rename する。
pub fn write_mode(path: &Path, mode: Mode) -> Result<()> {
    let content = format!("mode = \"{}\"\n", mode.as_str());

    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("invalid mode file path (no parent): {}", path.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    tmp.write_all(content.as_bytes())
        .with_context(|| format!("failed to write temp mode file: {}", tmp.path().display()))?;
    tmp.persist(path)
        .map_err(|e| anyhow!("failed to persist mode file to {}: {}", path.display(), e))?;
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
/// ファイル不在・[default] 不在・id 空 はすべて未設定 (`Ok(None)`) として扱う。
/// TOML 構文エラー等の本当のエラーは `Err` で伝播する。
pub fn load_devices(path: &Path) -> Result<Option<DefaultDevice>> {
    let content = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e)
                .with_context(|| format!("failed to read devices file: {}", path.display()))
        }
    };
    let parsed: DevicesFile = toml::from_str(&content)
        .with_context(|| format!("failed to parse devices file: {}", path.display()))?;
    let mut device = match parsed.default {
        Some(d) => d,
        None => return Ok(None),
    };
    device.id = device.id.trim().to_string();
    if device.id.is_empty() {
        return Ok(None);
    }
    Ok(Some(device))
}

/// devices ファイルの状態を表す。
/// `Configured`: id が trim 後非空の有効な [default] セクションあり。
/// `Unconfigured`: ファイル不在 or [default] 不在 or id 空 (初期セットアップ前など)。
/// `Malformed`: TOML として解析できない (ユーザーが手動編集ミスした場合等)。
#[derive(Debug)]
pub enum DeviceState {
    Configured(DefaultDevice),
    Unconfigured,
    Malformed(String),
}

/// .env 形式テキストを KEY=VALUE のマップにパースする。
/// 空行・'#' で始まる行はスキップ、それ以外で `=` が無い行はエラー。
pub fn parse_env_content(content: &str) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match trimmed.split_once('=') {
            Some((key, value)) => {
                map.insert(key.trim().to_string(), value.trim().to_string());
            }
            None => {
                return Err(anyhow!(
                    "invalid .env line {} (no '=' separator): {}",
                    idx + 1,
                    trimmed
                ));
            }
        }
    }
    Ok(map)
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
    let raw = match fs::read_to_string(env_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow!(".env file not found: {}", env_path.display()));
        }
        Err(e) => {
            return Err(e).with_context(|| format!("failed to read .env: {}", env_path.display()))
        }
    };
    let raw_map = parse_env_content(&raw)
        .with_context(|| format!("failed to parse {}", env_path.display()))?;

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
    parse_env_content(&stdout)
}

/// .env の permission を検証し、group/other に読み権限があれば自動で 0o600 相当に修正する。
fn enforce_env_permissions(path: &Path) -> Result<()> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    let current_mode = metadata.permissions().mode();
    if current_mode & 0o077 != 0 {
        // group/other に何らかの権限がある → 0o600 に強制
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to enforce 0o600 on {}", path.display()))?;
    }
    Ok(())
}

#[derive(Debug)]
pub struct Context {
    pub credentials: Credentials,
    pub device: DeviceState,
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
/// `.env` がなければテンプレを書き出して編集を促すエラーで返す。
/// `devices` がない・空 id の場合は `device = None` として list に進ませる。
pub fn load_context() -> std::result::Result<Context, LoadContextError> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| LoadContextError::Setup(anyhow!("ホームディレクトリを特定できません")))?;
    load_context_at(base.home_dir())
}

/// load_context の内部実装。home_dir を受け取るため、テストで差し替え可能。
pub fn load_context_at(home_dir: &Path) -> std::result::Result<Context, LoadContextError> {
    let dir = home_dir.join(".switchbot");
    fs::create_dir_all(&dir).map_err(|e| {
        LoadContextError::Setup(
            anyhow::Error::new(e).context(format!("failed to create {}", dir.display())),
        )
    })?;

    let env_path = dir.join(".env");
    let devices_path = dir.join("devices");
    let mode_path = dir.join("mode");
    let log_path = dir.join("log");

    // .env テンプレ作成 (新規時のみ)
    let env_created = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&env_path)
    {
        Ok(mut f) => {
            f.write_all(ENV_TEMPLATE.as_bytes()).map_err(|e| {
                LoadContextError::Setup(
                    anyhow::Error::new(e)
                        .context(format!("failed to write template: {}", env_path.display())),
                )
            })?;
            fs::set_permissions(&env_path, fs::Permissions::from_mode(0o600)).map_err(|e| {
                LoadContextError::Setup(
                    anyhow::Error::new(e)
                        .context(format!("failed to chmod 600: {}", env_path.display())),
                )
            })?;
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => {
            return Err(LoadContextError::Setup(
                anyhow::Error::new(e).context(format!("failed to create {}", env_path.display())),
            ));
        }
    };

    // devices テンプレ作成 (新規時のみ、ただし list で動くために exit はしない)
    let _devices_created = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&devices_path)
    {
        Ok(mut f) => {
            f.write_all(DEVICES_TEMPLATE.as_bytes()).map_err(|e| {
                LoadContextError::Setup(anyhow::Error::new(e).context(format!(
                    "failed to write template: {}",
                    devices_path.display()
                )))
            })?;
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => {
            return Err(LoadContextError::Setup(
                anyhow::Error::new(e)
                    .context(format!("failed to create {}", devices_path.display())),
            ));
        }
    };

    // .env が新規作成された場合は編集を促して exit。これは Setup。
    if env_created {
        return Err(LoadContextError::Setup(anyhow!(
            "{} を編集してください",
            env_path.display()
        )));
    }

    // .env の権限を検証し、必要なら 0o600 に自動修正 (既存ファイルへの操作なので Runtime)
    enforce_env_permissions(&env_path).map_err(LoadContextError::Runtime)?;

    // credentials ロード (op inject 失敗等は Runtime)
    let credentials = load_credentials(&env_path).map_err(LoadContextError::Runtime)?;

    // devices ファイルが存在しても空 id のままなら Unconfigured として list に進ませる
    // TOML parse error は Malformed として記録するが、全体 fail させない
    let device = match load_devices(&devices_path) {
        Ok(Some(d)) => DeviceState::Configured(d),
        Ok(None) => DeviceState::Unconfigured,
        Err(e) => DeviceState::Malformed(format!("{:#}", e)),
    };

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
        let device = load_devices(&path).unwrap().unwrap();
        assert_eq!(device.id, "01-202311241234-12345678");
        assert_eq!(device.r#type, "Color Bulb");
        assert_eq!(device.name, "Living Bulb");
    }

    #[test]
    fn load_devices_missing_file_returns_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        assert!(load_devices(&path).unwrap().is_none());
    }

    #[test]
    fn load_devices_missing_default_section_returns_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        fs::write(
            &path,
            r#"[other]
id = "x"
"#,
        )
        .unwrap();
        assert!(load_devices(&path).unwrap().is_none());
    }

    #[test]
    fn load_devices_empty_id_returns_none() {
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
        assert!(load_devices(&path).unwrap().is_none());
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
        let device = load_devices(&path).unwrap().unwrap();
        assert_eq!(device.id, "abc");
        assert_eq!(device.r#type, "");
    }

    #[test]
    fn load_devices_malformed_toml_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        fs::write(&path, "this is === not toml ===").unwrap();
        assert!(load_devices(&path).is_err());
    }

    #[test]
    fn parse_env_basic() {
        let content = "FOO=bar\nBAZ=qux\n";
        let map = parse_env_content(content).unwrap();
        assert_eq!(map.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(map.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    fn parse_env_skips_comments_and_blank() {
        let content = "\n# comment\nFOO=bar\n\n# another\nBAZ=qux\n";
        let map = parse_env_content(content).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn parse_env_trims_whitespace() {
        let content = "  FOO  =  bar  \n";
        let map = parse_env_content(content).unwrap();
        assert_eq!(map.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn parse_env_keeps_op_reference_literally() {
        let content = "TOKEN=op://Personal/Item/credential\n";
        let map = parse_env_content(content).unwrap();
        assert_eq!(
            map.get("TOKEN"),
            Some(&"op://Personal/Item/credential".to_string())
        );
    }

    #[test]
    fn parse_env_rejects_malformed_line() {
        let result = parse_env_content("SWITCHBOT_TOKEN tok\n");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("line 1"), "msg={}", msg);
        assert!(msg.contains("no '=' separator"), "msg={}", msg);
    }

    #[test]
    fn parse_env_keeps_silent_for_valid_input() {
        let result = parse_env_content("FOO=bar\n# comment\n\nBAZ=qux\n");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
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

    // --- 修正 1 テスト: load_context_at ---

    fn make_valid_env(cfg_dir: &Path) {
        let env_path = cfg_dir.join(".env");
        fs::write(&env_path, "SWITCHBOT_TOKEN=tok\nSWITCHBOT_SECRET=sec\n").unwrap();
        fs::set_permissions(&env_path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn load_context_with_empty_devices_yields_unconfigured() {
        let dir = tempdir().unwrap();
        let cfg_dir = dir.path().join(".switchbot");
        fs::create_dir_all(&cfg_dir).unwrap();
        make_valid_env(&cfg_dir);
        fs::write(
            cfg_dir.join("devices"),
            "[default]\nid = \"\"\ntype = \"Color Bulb\"\n",
        )
        .unwrap();

        let ctx = load_context_at(dir.path()).unwrap();
        assert!(matches!(ctx.device, DeviceState::Unconfigured));
    }

    #[test]
    fn load_context_with_missing_devices_yields_unconfigured() {
        let dir = tempdir().unwrap();
        let cfg_dir = dir.path().join(".switchbot");
        fs::create_dir_all(&cfg_dir).unwrap();
        make_valid_env(&cfg_dir);
        // devices ファイルなし → テンプレが作られ Unconfigured

        let ctx = load_context_at(dir.path()).unwrap();
        assert!(matches!(ctx.device, DeviceState::Unconfigured));
    }

    #[test]
    fn load_context_with_valid_devices_yields_configured() {
        let dir = tempdir().unwrap();
        let cfg_dir = dir.path().join(".switchbot");
        fs::create_dir_all(&cfg_dir).unwrap();
        make_valid_env(&cfg_dir);
        fs::write(
            cfg_dir.join("devices"),
            "[default]\nid = \"abc-123\"\ntype = \"Color Bulb\"\n",
        )
        .unwrap();

        let ctx = load_context_at(dir.path()).unwrap();
        match ctx.device {
            DeviceState::Configured(d) => assert_eq!(d.id, "abc-123"),
            _ => panic!("expected Configured"),
        }
    }

    #[test]
    fn load_context_with_malformed_devices_yields_malformed() {
        let dir = tempdir().unwrap();
        let cfg_dir = dir.path().join(".switchbot");
        fs::create_dir_all(&cfg_dir).unwrap();
        // .env は正常 (op:// なしの平文)
        fs::write(
            cfg_dir.join(".env"),
            "SWITCHBOT_TOKEN=tok\nSWITCHBOT_SECRET=sec\n",
        )
        .unwrap();
        fs::set_permissions(cfg_dir.join(".env"), fs::Permissions::from_mode(0o600)).unwrap();
        // devices は壊れた TOML
        fs::write(cfg_dir.join("devices"), "malformed === not toml ===").unwrap();

        // malformed devices は全体 fail しない (Err ではなく Ok with Malformed)
        let ctx = load_context_at(dir.path()).unwrap();
        assert!(
            matches!(ctx.device, DeviceState::Malformed(_)),
            "expected Malformed"
        );
    }

    #[test]
    fn load_context_new_env_requires_edit() {
        // .env が存在しない場合は "編集してください" エラーが返る
        let dir = tempdir().unwrap();
        let err = load_context_at(dir.path()).unwrap_err();
        assert!(err.to_string().contains("編集してください"));
    }

    #[test]
    fn load_context_first_run_is_setup_error() {
        // ファイル何もない状態 → bootstrap で .env テンプレが書き出される (Setup)
        let dir = tempdir().unwrap();
        let result = load_context_at(dir.path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            !err.should_notify(),
            "first-run bootstrap should be Setup (no notify)"
        );
    }

    // --- 修正 2 テスト: enforce_env_permissions ---

    #[test]
    fn enforce_env_permissions_tightens_loose_perms() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "SWITCHBOT_TOKEN=x\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        enforce_env_permissions(&path).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "expected 0o600 after enforcement, got {:o}",
            mode
        );
    }

    #[test]
    fn enforce_env_permissions_keeps_strict_perms() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "SWITCHBOT_TOKEN=x\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        enforce_env_permissions(&path).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn load_devices_whitespace_only_id_returns_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        fs::write(&path, "[default]\nid = \"   \"\ntype = \"Color Bulb\"\n").unwrap();
        let result = load_devices(&path).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn load_devices_id_is_trimmed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        fs::write(&path, "[default]\nid = \"  abc  \"\n").unwrap();
        let result = load_devices(&path).unwrap().unwrap();
        assert_eq!(result.id, "abc");
    }
}
