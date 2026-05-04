# switchbot-rs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** SwitchBot Color Bulb を Stream Deck から操作する Rust 製シングルバイナリ CLI を v2 設計どおりに実装する。

**Architecture:** clap でパースした引数を `commands` モジュールが API/モードファイルと対話して実行し、結果を `feedback` モジュールがログ + osascript 通知に流す。値はすべて SwitchBot v1.1 API の GET status を真実の情報源とし、ローカルにはモード 1 ビットのみを保持する。認証は `~/.switchbot/.env` 経由で 1Password (`op inject`) か平文値から解決する。

**Tech Stack:** Rust 1.70+ / clap 4 (derive) / reqwest 0.12 (rustls + blocking) / serde + toml + serde_json / hmac + sha2 + base64 / uuid / directories / anyhow / chrono。dev: tempfile。1Password CLI (`op`) は `op://` 参照を使う場合のみ必須。

---

## File Structure

```
switchbot-rs/
  Cargo.toml
  Cargo.lock
  .gitignore
  README.md                         # Task 16 で追加
  docs/SPEC.md                      # 既存。実装側からは参照のみ
  docs/superpowers/
    specs/2026-05-04-switchbot-rs-design.md  # 既存 (v2 設計)
    plans/2026-05-05-switchbot-rs-implementation.md  # 本書
  src/
    main.rs                         # エントリ。clap parse → load_context → dispatch → feedback。
    cli.rs                          # clap derive: Cli, Command, BumpAxis。引数バリデーションも担う。
    config.rs                       # ~/.switchbot/{.env, devices, mode} の読み書き、bootstrap、Context 組み立て。
    feedback.rs                     # ログ append + osascript 通知 + stderr 出力。
    commands.rs                     # サブコマンドハンドラ: cmd_color/bright/temp/bump/on/off/list と bump 算術。
    api/
      mod.rs                        # Client、ApiResponse、Device、BulbStatus、公開操作メソッド。
      signing.rs                    # private: HMAC-SHA256 + base64 + uppercase の純粋関数。
```

各ファイルの責務:
- `cli.rs`: 入力値の構文チェック (hex 6 桁、temp 範囲、bright 1–100|max、bump axis 列挙) のみ。意味的判定は `commands.rs`。
- `config.rs`: ファイルパスの組立、I/O、TOML/.env のパース、`op inject` 起動、初回 bootstrap。
- `commands.rs`: 各サブコマンドのビジネスロジック (モード判定、API 呼び出し、結果文字列の組立)。
- `api/mod.rs`: HTTP クライアント。エンドポイントごとの公開関数で構成。signing は `api::signing` で隠蔽。
- `feedback.rs`: 「成功・失敗 → ログ・通知・stderr」のディスパッチを 1 箇所に集約。
- `main.rs`: 上記をつなぐ。エラー時は exit 1、成功時は exit 0。

---

## Task 1: Cargo プロジェクト初期化と依存追加

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `.gitignore`

- [ ] **Step 1: cargo init で雛形生成**

```bash
cd /Users/ryugo/Developer/src/personal/switchbot-rs
cargo init --vcs none --name switchbot
```

`Cargo.toml` と `src/main.rs` (`fn main() { println!("Hello, world!"); }`) が生成される。`--vcs none` は既存の `.git` を尊重するため。

- [ ] **Step 2: Cargo.toml を編集して依存を追加**

`Cargo.toml` の内容を以下に置き換える:

```toml
[package]
name = "switchbot"
version = "0.1.0"
edition = "2021"

[dependencies]
clap        = { version = "4", features = ["derive"] }
reqwest     = { version = "0.12", features = ["rustls-tls", "blocking", "json"], default-features = false }
serde       = { version = "1", features = ["derive"] }
serde_json  = "1"
toml        = "0.8"
hmac        = "0.12"
sha2        = "0.10"
base64      = "0.22"
uuid        = { version = "1", features = ["v4"] }
directories = "5"
anyhow      = "1"
chrono      = "0.4"

[dev-dependencies]
tempfile    = "3"

[profile.release]
strip = true
lto   = "thin"
```

- [ ] **Step 3: .gitignore に target/ を追加**

`.gitignore` を作成:

```
/target
```

`Cargo.lock` はバイナリアプリのためコミットに含める (除外しない)。

- [ ] **Step 4: ビルドが通ることを確認**

```bash
cargo build
```

期待: `Finished dev [unoptimized + debuginfo] target(s)` で終了。依存解決とコンパイルが通る。

- [ ] **Step 5: コミット**

```bash
git add Cargo.toml Cargo.lock .gitignore src/main.rs
git commit -m "chore: cargo project bootstrap with dependencies"
```

---

## Task 2: CLI 引数パース (cli.rs)

**Files:**
- Create: `src/cli.rs`
- Modify: `src/main.rs` (mod 宣言追加のみ)

- [ ] **Step 1: 失敗するテストファースト**

`src/cli.rs` を新規作成し、以下を書く:

```rust
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "switchbot", about = "SwitchBot Color Bulb CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Color {
        #[arg(value_parser = parse_hex)]
        rgb: (u8, u8, u8),
    },
    Bright {
        #[arg(value_parser = parse_brightness)]
        value: u32,
    },
    Temp {
        #[arg(value_parser = parse_temperature)]
        kelvin: u32,
    },
    Bump {
        axis: BumpAxis,
    },
    On,
    Off,
    List,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum BumpAxis {
    #[value(name = "R+")]      RPlus,
    #[value(name = "R-")]      RMinus,
    #[value(name = "G+")]      GPlus,
    #[value(name = "G-")]      GMinus,
    #[value(name = "B+")]      BPlus,
    #[value(name = "B-")]      BMinus,
    #[value(name = "bright+")] BrightPlus,
    #[value(name = "bright-")] BrightMinus,
    #[value(name = "temp+")]   TempPlus,
    #[value(name = "temp-")]   TempMinus,
}

fn parse_hex(s: &str) -> Result<(u8, u8, u8), String> {
    if s.len() != 6 {
        return Err(format!("hex は 6 桁である必要があります (got {} 桁)", s.len()));
    }
    if !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("hex は 16 進数である必要があります (got '{}')", s));
    }
    let r = u8::from_str_radix(&s[0..2], 16).map_err(|e| e.to_string())?;
    let g = u8::from_str_radix(&s[2..4], 16).map_err(|e| e.to_string())?;
    let b = u8::from_str_radix(&s[4..6], 16).map_err(|e| e.to_string())?;
    Ok((r, g, b))
}

fn parse_brightness(s: &str) -> Result<u32, String> {
    if s == "max" {
        return Ok(100);
    }
    let n: u32 = s.parse().map_err(|_| format!("整数または 'max' を指定してください (got '{}')", s))?;
    if !(1..=100).contains(&n) {
        return Err(format!("明るさは 1-100 の範囲です (got {})", n));
    }
    Ok(n)
}

fn parse_temperature(s: &str) -> Result<u32, String> {
    let n: u32 = s.parse().map_err(|_| format!("整数を指定してください (got '{}')", s))?;
    if !(2700..=6500).contains(&n) {
        return Err(format!("温度は 2700-6500 の範囲です (got {})", n));
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("switchbot").chain(args.iter().copied()))
    }

    #[test]
    fn color_uppercase_ok() {
        let cli = parse(&["color", "FEDFE1"]).unwrap();
        match cli.command {
            Command::Color { rgb } => assert_eq!(rgb, (0xFE, 0xDF, 0xE1)),
            _ => panic!("expected Color"),
        }
    }

    #[test]
    fn color_lowercase_ok() {
        let cli = parse(&["color", "fedfe1"]).unwrap();
        match cli.command {
            Command::Color { rgb } => assert_eq!(rgb, (0xFE, 0xDF, 0xE1)),
            _ => panic!("expected Color"),
        }
    }

    #[test]
    fn color_with_hash_rejected() {
        assert!(parse(&["color", "#FEDFE1"]).is_err());
    }

    #[test]
    fn color_short_rejected() {
        assert!(parse(&["color", "FED"]).is_err());
    }

    #[test]
    fn color_non_hex_rejected() {
        assert!(parse(&["color", "ZZZZZZ"]).is_err());
    }

    #[test]
    fn bright_max_means_100() {
        let cli = parse(&["bright", "max"]).unwrap();
        match cli.command {
            Command::Bright { value } => assert_eq!(value, 100),
            _ => panic!("expected Bright"),
        }
    }

    #[test]
    fn bright_50_ok() {
        let cli = parse(&["bright", "50"]).unwrap();
        match cli.command {
            Command::Bright { value } => assert_eq!(value, 50),
            _ => panic!("expected Bright"),
        }
    }

    #[test]
    fn bright_zero_rejected() {
        assert!(parse(&["bright", "0"]).is_err());
    }

    #[test]
    fn bright_101_rejected() {
        assert!(parse(&["bright", "101"]).is_err());
    }

    #[test]
    fn temp_3000_ok() {
        let cli = parse(&["temp", "3000"]).unwrap();
        match cli.command {
            Command::Temp { kelvin } => assert_eq!(kelvin, 3000),
            _ => panic!("expected Temp"),
        }
    }

    #[test]
    fn temp_2699_rejected() {
        assert!(parse(&["temp", "2699"]).is_err());
    }

    #[test]
    fn temp_6501_rejected() {
        assert!(parse(&["temp", "6501"]).is_err());
    }

    #[test]
    fn bump_r_plus_ok() {
        let cli = parse(&["bump", "R+"]).unwrap();
        match cli.command {
            Command::Bump { axis } => assert_eq!(axis, BumpAxis::RPlus),
            _ => panic!("expected Bump"),
        }
    }

    #[test]
    fn bump_temp_minus_ok() {
        let cli = parse(&["bump", "temp-"]).unwrap();
        match cli.command {
            Command::Bump { axis } => assert_eq!(axis, BumpAxis::TempMinus),
            _ => panic!("expected Bump"),
        }
    }

    #[test]
    fn bump_unknown_axis_rejected() {
        assert!(parse(&["bump", "Z+"]).is_err());
    }

    #[test]
    fn on_off_list_parse() {
        assert!(matches!(parse(&["on"]).unwrap().command, Command::On));
        assert!(matches!(parse(&["off"]).unwrap().command, Command::Off));
        assert!(matches!(parse(&["list"]).unwrap().command, Command::List));
    }
}
```

- [ ] **Step 2: src/main.rs に mod 宣言を追加**

`src/main.rs` を以下に置き換える:

```rust
mod cli;

fn main() {
    let _ = cli::Cli::try_parse_from(std::env::args());
}
```

`use clap::Parser;` を main.rs の先頭に追加 (try_parse_from のため)。

```rust
use clap::Parser;

mod cli;

fn main() {
    let _ = cli::Cli::try_parse_from(std::env::args());
}
```

- [ ] **Step 3: テスト実行 → 全部 PASS することを確認**

```bash
cargo test --lib cli::
```

期待: 16 件のテストが全部 pass。コードと並行して書いたので「失敗 → 実装 → 成功」の TDD ループが厳密ではないが、テストファーストの体裁は保ちつつ動作確認する。

問題なし: clap derive と value_parser でパース時に検証するので、実装とテストが同時に通る構造。

- [ ] **Step 4: コミット**

```bash
git add src/cli.rs src/main.rs
git commit -m "feat(cli): add argument parsing with validation"
```

---

## Task 3: HMAC 署名関数 (api/signing.rs)

**Files:**
- Create: `src/api/mod.rs`
- Create: `src/api/signing.rs`
- Modify: `src/main.rs` (mod 宣言追加)

- [ ] **Step 1: api モジュール骨格と signing.rs**

`src/api/mod.rs` を作成:

```rust
mod signing;
```

`src/api/signing.rs` を作成:

```rust
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// SwitchBot v1.1 仕様の sign ヘッダ値を計算する。
///
/// `base64(HMAC-SHA256(token + t + nonce, secret))` を全大文字化して返す。
pub fn compute_sign(token: &str, secret: &str, t: i64, nonce: &str) -> String {
    let data = format!("{}{}{}", token, t, nonce);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(data.as_bytes());
    let result = mac.finalize().into_bytes();
    base64::engine::general_purpose::STANDARD
        .encode(result)
        .to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let a = compute_sign("token", "secret", 1234567890, "nonce");
        let b = compute_sign("token", "secret", 1234567890, "nonce");
        assert_eq!(a, b);
    }

    #[test]
    fn different_input_different_output() {
        let a = compute_sign("token1", "secret", 1234567890, "nonce");
        let b = compute_sign("token2", "secret", 1234567890, "nonce");
        assert_ne!(a, b);
    }

    #[test]
    fn output_has_no_lowercase() {
        let s = compute_sign("token", "secret", 1234567890, "nonce");
        assert!(
            s.chars().all(|c| !c.is_ascii_lowercase()),
            "expected no lowercase letters, got: {}",
            s
        );
    }

    #[test]
    fn output_length_44() {
        // base64 of 32 bytes (HMAC-SHA256 output) = 44 chars including padding
        let s = compute_sign("token", "secret", 1234567890, "nonce");
        assert_eq!(s.len(), 44);
    }

    #[test]
    fn matches_reference_python_vector() {
        // Reference value computed via:
        //   python3 -c "import hmac, hashlib, base64; \
        //     data=b'test_token1635146797241test-nonce'; \
        //     k=b'test_secret'; \
        //     print(base64.b64encode(hmac.new(k, data, hashlib.sha256).digest()).decode().upper())"
        let expected = "T0/YPYSTPMPKAS1VEE+VCYNNOSK+2V2ECQX6OTHADPU=";
        let actual = compute_sign("test_token", "test_secret", 1635146797241, "test-nonce");
        assert_eq!(actual, expected);
    }
}
```

- [ ] **Step 2: src/main.rs に mod api を追加**

`src/main.rs` を以下に変更:

```rust
use clap::Parser;

mod api;
mod cli;

fn main() {
    let _ = cli::Cli::try_parse_from(std::env::args());
}
```

- [ ] **Step 3: テスト実行 (known vector 以外) → PASS 確認**

```bash
cargo test --lib api::signing::tests::deterministic
cargo test --lib api::signing::tests::different_input_different_output
cargo test --lib api::signing::tests::output_has_no_lowercase
cargo test --lib api::signing::tests::output_length_44
```

期待: 4 件 pass。

- [ ] **Step 4: known vector の expected を計算して埋める**

ターミナルで実行:

```bash
python3 -c "import hmac, hashlib, base64; \
  data=b'test_token1635146797241test-nonce'; \
  k=b'test_secret'; \
  print(base64.b64encode(hmac.new(k, data, hashlib.sha256).digest()).decode().upper())"
```

得られた 44 文字を `src/api/signing.rs` の `matches_reference_python_vector` テスト内の `expected = "PASTE_OUTPUT_HERE"` を置き換える。

- [ ] **Step 5: known vector テスト実行 → PASS 確認**

```bash
cargo test --lib api::signing::tests::matches_reference_python_vector
```

期待: pass。fail する場合は実装が python と乖離している可能性。`compute_sign` 内の data 連結順 (token + t + nonce) と base64 encode の方式 (STANDARD = `+/=` を含む通常の base64) を確認。

- [ ] **Step 6: 全 signing テスト実行**

```bash
cargo test --lib api::signing
```

期待: 5 件 pass。

- [ ] **Step 7: コミット**

```bash
git add src/api/mod.rs src/api/signing.rs src/main.rs
git commit -m "feat(api): add HMAC-SHA256 signing for SwitchBot v1.1"
```

---

## Task 4: モードファイル read/write (config.rs partial)

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs` (mod 宣言追加)

- [ ] **Step 1: テスト + 実装 (mode 部分のみ)**

`src/config.rs` を作成:

```rust
use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Rgb,
    Temp,
}

#[derive(Serialize, Deserialize)]
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
        other => Err(anyhow!("invalid mode value '{}': expected 'rgb' or 'temp'", other)),
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
}
```

- [ ] **Step 2: src/main.rs に mod config を追加**

```rust
use clap::Parser;

mod api;
mod cli;
mod config;

fn main() {
    let _ = cli::Cli::try_parse_from(std::env::args());
}
```

- [ ] **Step 3: テスト実行**

```bash
cargo test --lib config::tests
```

期待: 5 件 pass。

- [ ] **Step 4: コミット**

```bash
git add src/config.rs src/main.rs
git commit -m "feat(config): add mode file read/write"
```

---

## Task 5: devices ファイル load (config.rs 追記)

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: テスト + 実装**

`src/config.rs` の末尾の `#[cfg(test)]` の **直前** に以下を追加:

```rust
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
    let device = parsed.default
        .ok_or_else(|| anyhow!("[default] section not found in {}", path.display()))?;
    if device.id.is_empty() {
        return Err(anyhow!("[default] id is empty in {}", path.display()));
    }
    Ok(device)
}
```

`src/config.rs` の `mod tests` の中に以下を追加:

```rust
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
        ).unwrap();
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
        fs::write(&path, r#"[other]
id = "x"
"#).unwrap();
        assert!(load_devices(&path).is_err());
    }

    #[test]
    fn load_devices_empty_id_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        fs::write(&path, r#"[default]
id = ""
type = "Color Bulb"
"#).unwrap();
        assert!(load_devices(&path).is_err());
    }

    #[test]
    fn load_devices_id_only_minimum_ok() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        fs::write(&path, r#"[default]
id = "abc"
"#).unwrap();
        let device = load_devices(&path).unwrap();
        assert_eq!(device.id, "abc");
        assert_eq!(device.r#type, "");
    }
```

- [ ] **Step 2: テスト実行**

```bash
cargo test --lib config::tests
```

期待: 計 10 件 pass (Task 4 の 5 件 + 新規 5 件)。

- [ ] **Step 3: コミット**

```bash
git add src/config.rs
git commit -m "feat(config): add devices file loading"
```

---

## Task 6: .env パースと op:// 検出 (config.rs 追記)

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: テスト + 実装**

`src/config.rs` の `mod tests` の **直前** に以下を追加:

```rust
use std::collections::HashMap;

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
```

`mod tests` の中に以下を追加:

```rust
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
        assert_eq!(map.get("TOKEN"), Some(&"op://Personal/Item/credential".to_string()));
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
```

- [ ] **Step 2: テスト実行**

```bash
cargo test --lib config::tests
```

期待: 計 16 件 pass。

- [ ] **Step 3: コミット**

```bash
git add src/config.rs
git commit -m "feat(config): add .env parsing and op:// reference detection"
```

---

## Task 7: 1Password 解決と認証情報のロード (config.rs 追記)

**Files:**
- Modify: `src/config.rs`

`op inject` を起動する関数は子プロセス起動を伴うのでユニットテストしない。代わりにロジック (引数バリデーション、エラーマッピング) は `load_credentials` 内で個別関数を組み合わせる構造にし、`op inject` を呼ぶ薄い関数だけが untestable にする。

- [ ] **Step 1: 認証情報ロード関数を追加**

`src/config.rs` の `mod tests` の **直前** に以下を追加:

```rust
use std::process::Command;

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
        .ok_or_else(|| anyhow!("SWITCHBOT_TOKEN is empty or missing in {}", env_path.display()))?
        .clone();
    let secret = resolved
        .get("SWITCHBOT_SECRET")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("SWITCHBOT_SECRET is empty or missing in {}", env_path.display()))?
        .clone();

    Ok(Credentials { token, secret })
}

fn resolve_with_op_inject(env_path: &Path) -> Result<HashMap<String, String>> {
    let output = Command::new("op")
        .arg("inject")
        .arg("-i")
        .arg(env_path)
        .output()
        .map_err(|e| anyhow!(
            "failed to execute `op inject`: {}. Is the 1Password CLI (`op`) installed and on PATH?",
            e
        ))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "`op inject` failed (1Password unlock 状態を確認してください): {}",
            stderr.trim()
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .context("`op inject` returned non-UTF8 output")?;
    Ok(parse_env_content(&stdout))
}
```

`mod tests` の中に以下を追加 (op inject を呼ばない経路のテスト):

```rust
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
```

注意: `load_credentials_op_path_*` は `op` コマンドの実行を伴うのでユニットテスト対象外 (手動 smoke でカバー)。

- [ ] **Step 2: テスト実行**

```bash
cargo test --lib config::tests
```

期待: 計 20 件 pass。

- [ ] **Step 3: コミット**

```bash
git add src/config.rs
git commit -m "feat(config): add 1Password credential resolution via op inject"
```

---

## Task 8: 設定ディレクトリの bootstrap (config.rs 追記)

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: bootstrap 関数と Context 構造体を追加**

`src/config.rs` の `mod tests` の **直前** に以下を追加:

```rust
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
        needs_setup.push(format!("{} を編集してください (switchbot list で deviceId を確認)", devices_path.display()));
    }

    if !needs_setup.is_empty() {
        return Err(anyhow!("{}", needs_setup.join("\n")));
    }

    let credentials = load_credentials(&env_path)?;
    let device = load_devices(&devices_path)?;

    Ok(Context { credentials, device, mode_path, log_path })
}
```

`mod tests` の中に以下を追加:

```rust
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
```

`config_dir()` と `load_context()` 自体は `$HOME` に依存するためユニットテストしない (手動 smoke でカバー)。テンプレ文字列の妥当性のみ静的に検証。

- [ ] **Step 2: テスト実行**

```bash
cargo test --lib config::tests
```

期待: 計 22 件 pass。

- [ ] **Step 3: コミット**

```bash
git add src/config.rs
git commit -m "feat(config): add bootstrap with templates and Context struct"
```

---

## Task 9: API レスポンス型と JSON パース (api/mod.rs 追記)

**Files:**
- Modify: `src/api/mod.rs`

- [ ] **Step 1: 型定義とパーステスト**

`src/api/mod.rs` を以下に置き換える:

```rust
mod signing;

use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct ApiResponse<T> {
    #[serde(rename = "statusCode")]
    pub status_code: i64,
    pub message: String,
    pub body: Option<T>,
}

#[derive(Deserialize, Debug)]
pub struct DeviceList {
    #[serde(rename = "deviceList")]
    pub device_list: Vec<Device>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Device {
    #[serde(rename = "deviceId")]
    pub id: String,
    #[serde(rename = "deviceName")]
    pub name: String,
    #[serde(rename = "deviceType")]
    pub kind: String,
}

#[derive(Deserialize, Debug)]
pub struct BulbStatus {
    pub power: String,
    pub brightness: u32,
    pub color: String,
    #[serde(rename = "colorTemperature")]
    pub color_temperature: u32,
}

/// "R:G:B" 形式の文字列を分解する。
pub fn parse_color_str(s: &str) -> anyhow::Result<(u8, u8, u8)> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        anyhow::bail!("invalid color string '{}': expected 'R:G:B'", s);
    }
    let r: u8 = parts[0].parse().map_err(|_| anyhow::anyhow!("invalid R in '{}'", s))?;
    let g: u8 = parts[1].parse().map_err(|_| anyhow::anyhow!("invalid G in '{}'", s))?;
    let b: u8 = parts[2].parse().map_err(|_| anyhow::anyhow!("invalid B in '{}'", s))?;
    Ok((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_device_list_response() {
        let json = r#"{
            "statusCode": 100,
            "message": "success",
            "body": {
                "deviceList": [
                    {"deviceId": "01-x", "deviceName": "Living Bulb", "deviceType": "Color Bulb"},
                    {"deviceId": "02-y", "deviceName": "Bedroom Plug", "deviceType": "Plug Mini"}
                ]
            }
        }"#;
        let parsed: ApiResponse<DeviceList> = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.status_code, 100);
        let body = parsed.body.unwrap();
        assert_eq!(body.device_list.len(), 2);
        assert_eq!(body.device_list[0].id, "01-x");
        assert_eq!(body.device_list[0].name, "Living Bulb");
        assert_eq!(body.device_list[0].kind, "Color Bulb");
    }

    #[test]
    fn parse_bulb_status() {
        let json = r#"{
            "statusCode": 100,
            "message": "success",
            "body": {
                "power": "on",
                "brightness": 50,
                "color": "255:128:0",
                "colorTemperature": 0,
                "version": "V1.0"
            }
        }"#;
        let parsed: ApiResponse<BulbStatus> = serde_json::from_str(json).unwrap();
        let body = parsed.body.unwrap();
        assert_eq!(body.power, "on");
        assert_eq!(body.brightness, 50);
        assert_eq!(body.color, "255:128:0");
        assert_eq!(body.color_temperature, 0);
    }

    #[test]
    fn parse_color_str_basic() {
        assert_eq!(parse_color_str("255:128:0").unwrap(), (255, 128, 0));
        assert_eq!(parse_color_str("0:0:0").unwrap(), (0, 0, 0));
    }

    #[test]
    fn parse_color_str_invalid_count() {
        assert!(parse_color_str("255:128").is_err());
        assert!(parse_color_str("255:128:0:1").is_err());
    }

    #[test]
    fn parse_color_str_invalid_number() {
        assert!(parse_color_str("256:0:0").is_err());
        assert!(parse_color_str("foo:bar:baz").is_err());
    }
}
```

既存の `mod signing;` 行は新しい `src/api/mod.rs` の冒頭に保持される (置き換え版にも含まれている)。

- [ ] **Step 2: テスト実行**

```bash
cargo test --lib api::tests
```

期待: 5 件 pass。

- [ ] **Step 3: コミット**

```bash
git add src/api/mod.rs
git commit -m "feat(api): add response types and color string parser"
```

---

## Task 10: API クライアントと認証ヘッダ (api/mod.rs 追記)

**Files:**
- Modify: `src/api/mod.rs`

- [ ] **Step 1: Client 構造体と auth_headers 関数**

`src/api/mod.rs` の `mod tests` の **直前** に以下を追加:

```rust
use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use std::time::Duration;

const BASE_URL: &str = "https://api.switch-bot.com";

pub struct Client {
    pub token: String,
    pub secret: String,
    http: reqwest::blocking::Client,
}

impl Client {
    pub fn new(token: String, secret: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { token, secret, http })
    }

    fn auth_headers(&self) -> Result<HeaderMap> {
        let t = chrono::Utc::now().timestamp_millis();
        let nonce = uuid::Uuid::new_v4().to_string();
        let sign = signing::compute_sign(&self.token, &self.secret, t, &nonce);
        Self::build_headers(&self.token, t, &nonce, &sign)
    }

    fn build_headers(token: &str, t: i64, nonce: &str, sign: &str) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", HeaderValue::from_str(token)?);
        headers.insert("sign", HeaderValue::from_str(sign)?);
        headers.insert("t", HeaderValue::from_str(&t.to_string())?);
        headers.insert("nonce", HeaderValue::from_str(nonce)?);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }
}
```

`mod tests` の中に以下を追加:

```rust
    #[test]
    fn build_headers_contains_all_required() {
        let headers = Client::build_headers("tok", 1234567890, "noncex", "ABCD1234").unwrap();
        assert_eq!(headers.get("Authorization").unwrap(), "tok");
        assert_eq!(headers.get("sign").unwrap(), "ABCD1234");
        assert_eq!(headers.get("t").unwrap(), "1234567890");
        assert_eq!(headers.get("nonce").unwrap(), "noncex");
        assert_eq!(headers.get("content-type").unwrap(), "application/json");
    }

    #[test]
    fn client_new_with_5s_timeout_builds() {
        let client = Client::new("tok".to_string(), "sec".to_string());
        assert!(client.is_ok());
    }
```

- [ ] **Step 2: テスト実行**

```bash
cargo test --lib api::tests
```

期待: 計 7 件 pass。

- [ ] **Step 3: コミット**

```bash
git add src/api/mod.rs
git commit -m "feat(api): add HTTP client with 5s timeout and auth headers"
```

---

## Task 11: API 操作メソッド (api/mod.rs 追記)

**Files:**
- Modify: `src/api/mod.rs`

これらは実 HTTP を打つため自動テストしない (手動 smoke でカバー)。実装の正しさは Task 16 のチェックリストで検証する。

- [ ] **Step 1: 公開操作メソッドを追加**

`impl Client { ... }` の閉じ `}` の直前 (auth_headers の下) に以下を追加:

```rust
    pub fn list_devices(&self) -> Result<Vec<Device>> {
        let url = format!("{}/v1.1/devices", BASE_URL);
        let resp = self.http.get(&url)
            .headers(self.auth_headers()?)
            .send()
            .context("HTTP request failed (list_devices)")?;
        let api: ApiResponse<DeviceList> = resp
            .error_for_status()
            .context("HTTP error from list_devices")?
            .json()
            .context("failed to decode list_devices JSON")?;
        check_status(&api)?;
        Ok(api.body.context("empty body in list_devices")?.device_list)
    }

    pub fn get_status(&self, device_id: &str) -> Result<BulbStatus> {
        let url = format!("{}/v1.1/devices/{}/status", BASE_URL, device_id);
        let resp = self.http.get(&url)
            .headers(self.auth_headers()?)
            .send()
            .context("HTTP request failed (get_status)")?;
        let api: ApiResponse<BulbStatus> = resp
            .error_for_status()
            .context("HTTP error from get_status")?
            .json()
            .context("failed to decode get_status JSON")?;
        check_status(&api)?;
        api.body.context("empty body in get_status")
    }

    fn send_command(&self, device_id: &str, command: &str, parameter: &str) -> Result<()> {
        let url = format!("{}/v1.1/devices/{}/commands", BASE_URL, device_id);
        let body = serde_json::json!({
            "command": command,
            "parameter": parameter,
            "commandType": "command",
        });
        let resp = self.http.post(&url)
            .headers(self.auth_headers()?)
            .json(&body)
            .send()
            .context("HTTP request failed (send_command)")?;
        let api: ApiResponse<serde_json::Value> = resp
            .error_for_status()
            .context("HTTP error from send_command")?
            .json()
            .context("failed to decode send_command JSON")?;
        check_status(&api)?;
        Ok(())
    }

    pub fn set_color(&self, device_id: &str, r: u8, g: u8, b: u8) -> Result<()> {
        self.send_command(device_id, "setColor", &format!("{}:{}:{}", r, g, b))
    }

    pub fn set_brightness(&self, device_id: &str, value: u32) -> Result<()> {
        self.send_command(device_id, "setBrightness", &value.to_string())
    }

    pub fn set_color_temperature(&self, device_id: &str, kelvin: u32) -> Result<()> {
        self.send_command(device_id, "setColorTemperature", &kelvin.to_string())
    }

    pub fn turn_on(&self, device_id: &str) -> Result<()> {
        self.send_command(device_id, "turnOn", "default")
    }

    pub fn turn_off(&self, device_id: &str) -> Result<()> {
        self.send_command(device_id, "turnOff", "default")
    }
```

`use` セクション直下に以下のヘルパーを追加 (関数の外):

```rust
fn check_status<T>(resp: &ApiResponse<T>) -> Result<()> {
    if resp.status_code == 100 {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "API error: {} (statusCode={})",
            resp.message,
            resp.status_code
        ))
    }
}
```

`mod tests` 内に check_status のテストだけ追加 (純粋関数のため):

```rust
    #[test]
    fn check_status_100_ok() {
        let resp: ApiResponse<DeviceList> = serde_json::from_str(r#"{
            "statusCode": 100, "message": "success", "body": {"deviceList": []}
        }"#).unwrap();
        assert!(check_status(&resp).is_ok());
    }

    #[test]
    fn check_status_non_100_errors() {
        let resp: ApiResponse<DeviceList> = serde_json::from_str(r#"{
            "statusCode": 161, "message": "device offline", "body": null
        }"#).unwrap();
        let err = check_status(&resp).unwrap_err();
        assert!(err.to_string().contains("device offline"));
        assert!(err.to_string().contains("161"));
    }
```

- [ ] **Step 2: ビルドとテスト**

```bash
cargo build
cargo test --lib api::tests
```

期待: ビルド成功、テスト 9 件 pass。

- [ ] **Step 3: コミット**

```bash
git add src/api/mod.rs
git commit -m "feat(api): add list/status/command operations"
```

---

## Task 12: bump 算術関数 (commands.rs)

**Files:**
- Create: `src/commands.rs`
- Modify: `src/main.rs` (mod 宣言追加)

- [ ] **Step 1: bump 算術と clamp の純粋関数 + テスト**

`src/commands.rs` を新規作成:

```rust
use crate::cli::BumpAxis;

pub const RGB_STEP: i32 = 16;
pub const BRIGHT_STEP: i32 = 10;
pub const TEMP_STEP: i32 = 100;

pub fn clamp(value: i32, min: i32, max: i32) -> i32 {
    value.max(min).min(max)
}

pub fn bump_rgb_channel(current: u8, delta: i32) -> u8 {
    clamp(current as i32 + delta, 0, 255) as u8
}

pub fn bump_brightness(current: u32, delta: i32) -> u32 {
    clamp(current as i32 + delta, 1, 100) as u32
}

pub fn bump_temperature(current: u32, delta: i32) -> u32 {
    clamp(current as i32 + delta, 2700, 6500) as u32
}

/// axis から (axis_kind, signed_step) を返す。
pub fn axis_delta(axis: BumpAxis) -> AxisDelta {
    use BumpAxis::*;
    match axis {
        RPlus  => AxisDelta::Red(RGB_STEP),
        RMinus => AxisDelta::Red(-RGB_STEP),
        GPlus  => AxisDelta::Green(RGB_STEP),
        GMinus => AxisDelta::Green(-RGB_STEP),
        BPlus  => AxisDelta::Blue(RGB_STEP),
        BMinus => AxisDelta::Blue(-RGB_STEP),
        BrightPlus  => AxisDelta::Brightness(BRIGHT_STEP),
        BrightMinus => AxisDelta::Brightness(-BRIGHT_STEP),
        TempPlus    => AxisDelta::Temperature(TEMP_STEP),
        TempMinus   => AxisDelta::Temperature(-TEMP_STEP),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisDelta {
    Red(i32),
    Green(i32),
    Blue(i32),
    Brightness(i32),
    Temperature(i32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_basic() {
        assert_eq!(clamp(50, 0, 100), 50);
        assert_eq!(clamp(-5, 0, 100), 0);
        assert_eq!(clamp(150, 0, 100), 100);
        assert_eq!(clamp(0, 0, 100), 0);
        assert_eq!(clamp(100, 0, 100), 100);
    }

    #[test]
    fn bump_rgb_within_range() {
        assert_eq!(bump_rgb_channel(100, 16), 116);
        assert_eq!(bump_rgb_channel(100, -16), 84);
    }

    #[test]
    fn bump_rgb_clamps_at_max() {
        assert_eq!(bump_rgb_channel(250, 16), 255);
        assert_eq!(bump_rgb_channel(255, 16), 255);
    }

    #[test]
    fn bump_rgb_clamps_at_zero() {
        assert_eq!(bump_rgb_channel(10, -16), 0);
        assert_eq!(bump_rgb_channel(0, -16), 0);
    }

    #[test]
    fn bump_brightness_clamps_at_one() {
        assert_eq!(bump_brightness(5, -10), 1);
        assert_eq!(bump_brightness(1, -10), 1);
    }

    #[test]
    fn bump_brightness_clamps_at_100() {
        assert_eq!(bump_brightness(95, 10), 100);
        assert_eq!(bump_brightness(100, 10), 100);
    }

    #[test]
    fn bump_temperature_within_range() {
        assert_eq!(bump_temperature(3000, 100), 3100);
        assert_eq!(bump_temperature(3000, -100), 2900);
    }

    #[test]
    fn bump_temperature_clamps() {
        assert_eq!(bump_temperature(2750, -100), 2700);
        assert_eq!(bump_temperature(6450, 100), 6500);
        assert_eq!(bump_temperature(2700, -100), 2700);
        assert_eq!(bump_temperature(6500, 100), 6500);
    }

    #[test]
    fn axis_delta_mapping() {
        assert_eq!(axis_delta(BumpAxis::RPlus), AxisDelta::Red(16));
        assert_eq!(axis_delta(BumpAxis::RMinus), AxisDelta::Red(-16));
        assert_eq!(axis_delta(BumpAxis::BrightPlus), AxisDelta::Brightness(10));
        assert_eq!(axis_delta(BumpAxis::TempMinus), AxisDelta::Temperature(-100));
    }
}
```

- [ ] **Step 2: src/main.rs に mod commands を追加**

```rust
use clap::Parser;

mod api;
mod cli;
mod commands;
mod config;

fn main() {
    let _ = cli::Cli::try_parse_from(std::env::args());
}
```

- [ ] **Step 3: テスト実行**

```bash
cargo test --lib commands::tests
```

期待: 9 件 pass。

- [ ] **Step 4: コミット**

```bash
git add src/commands.rs src/main.rs
git commit -m "feat(commands): add bump arithmetic and axis-to-delta mapping"
```

---

## Task 13: サブコマンドハンドラ (commands.rs 追記)

**Files:**
- Modify: `src/commands.rs`

実 HTTP を打つ部分はテストしないが、`require_mode` のような純粋ロジックはテストする。

- [ ] **Step 1: モード判定とハンドラ実装**

`src/commands.rs` の `mod tests` の **直前** に以下を追加:

```rust
use anyhow::{anyhow, Result};

use crate::api::{self, parse_color_str, Client};
use crate::cli::{BumpAxis, Command};
use crate::config::{self, Context, Mode};

/// 期待モードと実際モードを照合し、ズレていればエラーを返す。
pub fn require_mode(actual: Option<Mode>, expected: Mode) -> Result<()> {
    match actual {
        None => Err(anyhow!(
            "モードが未設定です。先に switchbot color <hex> または switchbot temp <K> を実行してください。"
        )),
        Some(m) if m == expected => Ok(()),
        Some(_) => {
            let (current_label, switch_cmd) = match expected {
                Mode::Rgb => ("温度モード", "switchbot color <hex>"),
                Mode::Temp => ("RGB モード", "switchbot temp <K>"),
            };
            Err(anyhow!(
                "現在 {}です。{} を先に実行してください。",
                current_label,
                switch_cmd
            ))
        }
    }
}

pub fn handle(command: &Command, ctx: &Context) -> Result<String> {
    let client = Client::new(ctx.credentials.token.clone(), ctx.credentials.secret.clone())?;
    match command {
        Command::Color { rgb: (r, g, b) } => cmd_color(&client, ctx, *r, *g, *b),
        Command::Bright { value } => cmd_bright(&client, ctx, *value),
        Command::Temp { kelvin } => cmd_temp(&client, ctx, *kelvin),
        Command::Bump { axis } => cmd_bump(&client, ctx, *axis),
        Command::On => cmd_on(&client, ctx),
        Command::Off => cmd_off(&client, ctx),
        Command::List => cmd_list(&client),
    }
}

fn cmd_color(client: &Client, ctx: &Context, r: u8, g: u8, b: u8) -> Result<String> {
    client.set_color(&ctx.device.id, r, g, b)?;
    config::write_mode(&ctx.mode_path, Mode::Rgb)?;
    Ok(format!("color {:02X}{:02X}{:02X} ok", r, g, b))
}

fn cmd_bright(client: &Client, ctx: &Context, value: u32) -> Result<String> {
    client.set_brightness(&ctx.device.id, value)?;
    Ok(format!("bright {} ok", value))
}

fn cmd_temp(client: &Client, ctx: &Context, kelvin: u32) -> Result<String> {
    client.set_color_temperature(&ctx.device.id, kelvin)?;
    config::write_mode(&ctx.mode_path, Mode::Temp)?;
    Ok(format!("temp {} ok", kelvin))
}

fn cmd_on(client: &Client, ctx: &Context) -> Result<String> {
    client.turn_on(&ctx.device.id)?;
    Ok("on ok".to_string())
}

fn cmd_off(client: &Client, ctx: &Context) -> Result<String> {
    client.turn_off(&ctx.device.id)?;
    Ok("off ok".to_string())
}

fn cmd_list(client: &Client) -> Result<String> {
    let devices = client.list_devices()?;
    Ok(format_devices_toml(&devices))
}

fn cmd_bump(client: &Client, ctx: &Context, axis: BumpAxis) -> Result<String> {
    let mode = config::read_mode(&ctx.mode_path)?;
    let delta = axis_delta(axis);
    match delta {
        AxisDelta::Red(d) | AxisDelta::Green(d) | AxisDelta::Blue(d) => {
            require_mode(mode, Mode::Rgb)?;
            let status = client.get_status(&ctx.device.id)?;
            let (r0, g0, b0) = parse_color_str(&status.color)?;
            let (r, g, b) = match delta {
                AxisDelta::Red(_)   => (bump_rgb_channel(r0, d), g0, b0),
                AxisDelta::Green(_) => (r0, bump_rgb_channel(g0, d), b0),
                AxisDelta::Blue(_)  => (r0, g0, bump_rgb_channel(b0, d)),
                _ => unreachable!(),
            };
            client.set_color(&ctx.device.id, r, g, b)?;
            Ok(format!("bump {:?} ok ({}:{}:{})", axis, r, g, b))
        }
        AxisDelta::Brightness(d) => {
            let status = client.get_status(&ctx.device.id)?;
            let new_value = bump_brightness(status.brightness, d);
            client.set_brightness(&ctx.device.id, new_value)?;
            Ok(format!("bump {:?} ok ({})", axis, new_value))
        }
        AxisDelta::Temperature(d) => {
            require_mode(mode, Mode::Temp)?;
            let status = client.get_status(&ctx.device.id)?;
            let new_k = bump_temperature(status.color_temperature, d);
            client.set_color_temperature(&ctx.device.id, new_k)?;
            Ok(format!("bump {:?} ok ({}K)", axis, new_k))
        }
    }
}

/// list 出力用に Device 配列を TOML 形式に整形する。
/// 1 台なら [default]、複数なら deviceName を kebab-case 化したセクション名にする。
pub fn format_devices_toml(devices: &[api::Device]) -> String {
    let mut out = String::new();
    if devices.len() == 1 {
        let d = &devices[0];
        out.push_str("[default]\n");
        out.push_str(&format!("id = \"{}\"\n", d.id));
        out.push_str(&format!("type = \"{}\"\n", d.kind));
        out.push_str(&format!("name = \"{}\"\n", d.name));
    } else {
        for (i, d) in devices.iter().enumerate() {
            if i > 0 { out.push('\n'); }
            let key = sanitize_section_key(&d.name);
            out.push_str(&format!("[{}]\n", key));
            out.push_str(&format!("id = \"{}\"\n", d.id));
            out.push_str(&format!("type = \"{}\"\n", d.kind));
            out.push_str(&format!("name = \"{}\"\n", d.name));
        }
    }
    out
}

fn sanitize_section_key(name: &str) -> String {
    let lowered = name.to_lowercase();
    let mut key: String = lowered
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while key.contains("--") {
        key = key.replace("--", "-");
    }
    let trimmed = key.trim_matches('-').to_string();
    if trimmed.is_empty() { "device".to_string() } else { trimmed }
}
```

`mod tests` の中に以下を追加:

```rust
    use crate::config::Mode;

    #[test]
    fn require_mode_matches() {
        assert!(require_mode(Some(Mode::Rgb), Mode::Rgb).is_ok());
        assert!(require_mode(Some(Mode::Temp), Mode::Temp).is_ok());
    }

    #[test]
    fn require_mode_mismatch_rgb_expected() {
        let err = require_mode(Some(Mode::Temp), Mode::Rgb).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("温度モード"));
        assert!(msg.contains("switchbot color"));
    }

    #[test]
    fn require_mode_mismatch_temp_expected() {
        let err = require_mode(Some(Mode::Rgb), Mode::Temp).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("RGB モード"));
        assert!(msg.contains("switchbot temp"));
    }

    #[test]
    fn require_mode_none_errors() {
        let err = require_mode(None, Mode::Rgb).unwrap_err();
        assert!(err.to_string().contains("モードが未設定"));
    }

    #[test]
    fn sanitize_key_basic() {
        assert_eq!(sanitize_section_key("Living Bulb"), "living-bulb");
        assert_eq!(sanitize_section_key("Bedroom Plug Mini"), "bedroom-plug-mini");
        assert_eq!(sanitize_section_key("  Hi  "), "hi");
    }

    #[test]
    fn sanitize_key_collapses_separators() {
        assert_eq!(sanitize_section_key("A!!B__C"), "a-b-c");
    }

    #[test]
    fn sanitize_key_empty_falls_back() {
        assert_eq!(sanitize_section_key("---"), "device");
        assert_eq!(sanitize_section_key(""), "device");
    }

    #[test]
    fn format_devices_single_uses_default() {
        let devices = vec![api::Device {
            id: "01-x".to_string(),
            name: "Living Bulb".to_string(),
            kind: "Color Bulb".to_string(),
        }];
        let out = format_devices_toml(&devices);
        assert!(out.contains("[default]"));
        assert!(out.contains("id = \"01-x\""));
        assert!(out.contains("type = \"Color Bulb\""));
        assert!(out.contains("name = \"Living Bulb\""));
        assert!(!out.contains("[living-bulb]"));
    }

    #[test]
    fn format_devices_multi_uses_sanitized_keys() {
        let devices = vec![
            api::Device {
                id: "01-x".to_string(),
                name: "Living Bulb".to_string(),
                kind: "Color Bulb".to_string(),
            },
            api::Device {
                id: "02-y".to_string(),
                name: "Bedroom Plug".to_string(),
                kind: "Plug Mini".to_string(),
            },
        ];
        let out = format_devices_toml(&devices);
        assert!(out.contains("[living-bulb]"));
        assert!(out.contains("[bedroom-plug]"));
        assert!(!out.contains("[default]"));
    }
```

- [ ] **Step 2: ビルド + テスト実行**

```bash
cargo build
cargo test --lib commands::tests
```

期待: ビルド成功、テスト 18 件 pass (Task 12 の 9 件 + 新規 9 件)。

- [ ] **Step 3: コミット**

```bash
git add src/commands.rs
git commit -m "feat(commands): add subcommand handlers and list output formatter"
```

---

## Task 14: feedback (ログ + 通知)

**Files:**
- Create: `src/feedback.rs`
- Modify: `src/main.rs` (mod 宣言追加)

- [ ] **Step 1: feedback 実装とテスト**

`src/feedback.rs` を新規作成:

```rust
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::Command;

pub fn log_info(log_path: &Path, msg: &str) {
    write_log(log_path, "INFO", msg);
}

pub fn log_error(log_path: &Path, msg: &str) {
    write_log(log_path, "ERROR", msg);
}

fn write_log(log_path: &Path, level: &str, msg: &str) {
    let line = format_log_line(chrono::Local::now(), level, msg);
    let _ = OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

fn format_log_line(now: chrono::DateTime<chrono::Local>, level: &str, msg: &str) -> String {
    format!(
        "{} {:5} {}\n",
        now.format("%Y-%m-%dT%H:%M:%S%:z"),
        level,
        msg.lines().next().unwrap_or(""),
    )
}

pub fn notify(msg: &str) {
    let escaped = escape_for_applescript(msg);
    let _ = Command::new("osascript")
        .arg("-e")
        .arg(format!("display notification \"{}\" with title \"switchbot\"", escaped))
        .status();
}

fn escape_for_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::tempdir;

    #[test]
    fn log_appends_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("log");
        log_info(&path, "first message");
        log_info(&path, "second message");
        log_error(&path, "third message");
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("INFO"));
        assert!(lines[0].contains("first message"));
        assert!(lines[2].contains("ERROR"));
        assert!(lines[2].contains("third message"));
    }

    #[test]
    fn log_format_has_iso8601_timestamp() {
        let now = chrono::Local.with_ymd_and_hms(2026, 5, 4, 12, 34, 56).unwrap();
        let line = format_log_line(now, "INFO", "hello");
        assert!(line.starts_with("2026-05-04T12:34:56"));
        assert!(line.contains(" INFO  hello"));
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn log_strips_extra_lines_in_message() {
        let now = chrono::Local.with_ymd_and_hms(2026, 5, 4, 12, 34, 56).unwrap();
        let line = format_log_line(now, "ERROR", "first line\nsecond line");
        assert!(line.contains("first line"));
        assert!(!line.contains("second line"));
    }

    #[test]
    fn applescript_escape_doubles_backslash_and_quote() {
        assert_eq!(escape_for_applescript(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(escape_for_applescript("plain"), "plain");
    }
}
```

注: `notify` 自体は `osascript` 起動を伴うのでテストしない (`escape_for_applescript` のみテスト)。

- [ ] **Step 2: src/main.rs に mod feedback を追加**

```rust
use clap::Parser;

mod api;
mod cli;
mod commands;
mod config;
mod feedback;

fn main() {
    let _ = cli::Cli::try_parse_from(std::env::args());
}
```

- [ ] **Step 3: テスト実行**

```bash
cargo test --lib feedback::tests
```

期待: 4 件 pass。

- [ ] **Step 4: コミット**

```bash
git add src/feedback.rs src/main.rs
git commit -m "feat(feedback): add log file appender and macOS notifier"
```

---

## Task 15: main 配線

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: 配線とエラー分岐**

`src/main.rs` を以下に置き換える:

```rust
use clap::Parser;

mod api;
mod cli;
mod commands;
mod config;
mod feedback;

fn main() {
    let cli = cli::Cli::parse();
    let exit_code = match run(&cli) {
        Ok(()) => 0,
        Err(()) => 1,
    };
    std::process::exit(exit_code);
}

/// 戻り値の Err は単に「失敗した」を意味する。詳細メッセージは feedback で出力済み。
fn run(cli: &cli::Cli) -> Result<(), ()> {
    // 1) Context をロード。失敗 (HOME 不在、初回 bootstrap) は stderr のみ。通知/ログには出さない。
    let ctx = match config::load_context() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            return Err(());
        }
    };

    // 2) コマンドを実行。成功時はログ INFO、失敗時はログ ERROR + 通知 + stderr。
    match commands::handle(&cli.command, &ctx) {
        Ok(msg) => {
            feedback::log_info(&ctx.log_path, &msg);
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            feedback::log_error(&ctx.log_path, &msg);
            feedback::notify(&msg);
            eprintln!("{}", msg);
            Err(())
        }
    }
}
```

- [ ] **Step 2: ビルドと CLI ヘルプ確認**

```bash
cargo build --release
./target/release/switchbot --help
```

期待: clap が生成したヘルプが表示される。サブコマンド一覧 (`color`, `bright`, `temp`, `bump`, `on`, `off`, `list`) が見える。

- [ ] **Step 3: 全テスト実行**

```bash
cargo test --lib
```

期待: 全モジュールのテストが pass (合計 50+ 件)。

- [ ] **Step 4: コミット**

```bash
git add src/main.rs
git commit -m "feat(main): wire up bootstrap, dispatch, and feedback"
```

---

## Task 16: README と手動 smoke test チェックリスト

**Files:**
- Create: `README.md`

- [ ] **Step 1: README を書く**

`README.md` を新規作成:

```markdown
# switchbot-rs

SwitchBot Color Bulb (W1401400) を Stream Deck から操作するための Rust 製シングルバイナリ CLI。

## 特徴

- 単一バイナリ (~2-3 MB)。`~/.local/bin/switchbot` に置くだけ
- 認証情報は 1Password 連携 (`op://` 参照) または平文 `.env`
- bump コマンドで RGB/明るさ/色温度をワンタップずつ加減算
- macOS 通知でエラー時にユーザーへ即フィードバック

## インストール

```bash
cargo install --path . --root ~/.local
```

`~/.local/bin` を PATH に通すか、Stream Deck からはフルパス `$HOME/.local/bin/switchbot` を直接書く。

## 初回セットアップ

1. 初回起動するとテンプレが書き出される:
   ```bash
   switchbot list
   # → ~/.switchbot/.env と ~/.switchbot/devices のテンプレが作成される
   ```
2. `~/.switchbot/.env` を編集:
   ```
   SWITCHBOT_TOKEN=op://Personal/SwitchBot/token
   SWITCHBOT_SECRET=op://Personal/SwitchBot/secret
   ```
   (1Password 連携を使わない場合は値を直接記入)
3. デバイス一覧を取得して devices ファイルにリダイレクト:
   ```bash
   switchbot list > ~/.switchbot/devices
   ```
4. (複数台ある場合) 使うデバイスのセクション名を `[default]` にリネーム

## コマンド

```
switchbot color <hex>            # 例: switchbot color FEDFE1
switchbot bright <1-100|max>     # 例: switchbot bright 50
switchbot temp <2700-6500>       # 例: switchbot temp 3000
switchbot bump <axis>            # 例: switchbot bump R+
switchbot on
switchbot off
switchbot list
```

`bump` の axes:
- RGB: `R+`, `R-`, `G+`, `G-`, `B+`, `B-` (RGB モード時のみ。±16)
- 明るさ: `bright+`, `bright-` (両モード可。±10)
- 色温度: `temp+`, `temp-` (温度モード時のみ。±100K)

## モード drift について

別アプリ・別端末で電球のモードを変えると、本 CLI の `~/.switchbot/mode` は古い値のまま残ります。`bump R/G/B/temp±` がモード不一致エラーで弾かれたら、`switchbot color <hex>` または `switchbot temp <K>` を一度実行して再同期してください。

## ログ

- `~/.switchbot/log` に成功/失敗が 1 行ずつ追記される
- ローテーションなし。長期運用で大きくなったら手動で truncate してください

## 動作環境

- macOS (Apple Silicon / Intel)
- Rust 1.70+
- 1Password CLI (`op`) v2 以上 — `.env` で `op://` 参照を使う場合のみ必須
- 1Password アプリの biometric unlock を有効化推奨

## 開発

```bash
cargo build              # debug ビルド
cargo test               # 全ユニットテスト
cargo build --release    # リリースビルド
```

## 手動 smoke test (実機チェックリスト)

リリース前に実機で以下を順に実行し、目視確認すること。

- [ ] `switchbot list` で `[default]` に対象デバイスが見える
- [ ] `switchbot color FF0000` → 電球が赤
- [ ] `switchbot bump R-` → 赤味がやや下がる (R が 16 減って #EF0000 系)
- [ ] `switchbot bump G+` → 緑が 16 増える
- [ ] `switchbot bright 50` → 明るさ 50%
- [ ] `switchbot bump bright+` → 明るさが 60% に
- [ ] `switchbot temp 3000` → 暖色 3000K (温度モードへ切替)
- [ ] `switchbot bump R+` → エラー通知「現在 温度モードです」(モード不一致)
- [ ] `switchbot bump temp+` → 3100K に
- [ ] `switchbot bump bright-` → 明るさ -10% (温度モード中でも有効)
- [ ] `switchbot off` → 消灯
- [ ] `switchbot on` → 点灯
- [ ] `~/.switchbot/log` に各操作の INFO/ERROR が 1 行ずつ記録されている

## 仕様書

- 設計 (v2): `docs/superpowers/specs/2026-05-04-switchbot-rs-design.md`
- 実装計画: `docs/superpowers/plans/2026-05-05-switchbot-rs-implementation.md`
- 旧仕様 (v1, 参考): `docs/SPEC.md`
```

- [ ] **Step 2: ビルドが通り、ヘルプが README どおりに出ることを確認**

```bash
cargo build --release
./target/release/switchbot --help
./target/release/switchbot color --help
./target/release/switchbot bump --help
```

期待: 各サブコマンドのヘルプが期待どおりに表示される。

- [ ] **Step 3: コミット**

```bash
git add README.md
git commit -m "docs: add README with usage and smoke test checklist"
```

---

## Task 17: 全体最終確認

**Files:** なし (確認のみ)

- [ ] **Step 1: 全テストを実行**

```bash
cargo test --lib
```

期待: 全テスト pass。出力末尾の `test result: ok. NN passed; 0 failed` を確認。

- [ ] **Step 2: `cargo clippy` で lint チェック**

```bash
cargo clippy --all-targets -- -D warnings
```

期待: warning なしで完了。warning が出たら修正する (個別の対応は出た内容次第)。

- [ ] **Step 3: `cargo fmt` でフォーマット**

```bash
cargo fmt
git diff --stat
```

期待: フォーマット差分が出れば commit 対象。

```bash
git add -u
git commit -m "style: apply cargo fmt" || true
```

- [ ] **Step 4: リリースビルドと smoke test 実行**

```bash
cargo build --release
```

実機で `README.md` の手動 smoke test チェックリストを上から順に実行し、各項目を目視確認する。失敗があれば原因調査 (該当タスクに戻って修正)。

- [ ] **Step 5: インストール**

```bash
cargo install --path . --root ~/.local
~/.local/bin/switchbot --help
```

期待: インストール成功し、ヘルプが見える。

---

## 完了基準

- [ ] 全 16 タスクのチェックボックスが埋まっている
- [ ] `cargo test --lib` が全 pass
- [ ] `cargo clippy --all-targets -- -D warnings` が warning なし
- [ ] 実機 smoke test (README のチェックリスト) が全項目 pass
- [ ] `~/.local/bin/switchbot` がインストール済みで、Stream Deck から呼び出して動く

---

## 実装中の判断ガイド

- **テストが書きづらいと感じたら**: ロジックを純粋関数に切り出して、I/O は薄いラッパーに分離する (api 操作メソッドのように)
- **エラーメッセージで悩んだら**: ユーザーが Stream Deck の通知ポップアップで読むことを意識して、1 行で「何が」「どうすれば」を伝える
- **clap の derive で詰まったら**: `clap` 4.x のドキュメント (`#[arg(value_parser = ...)]`, `ValueEnum`) を確認
- **依存追加が必要になったら**: 設計書の依存リストに無いものは追加前に妥当性を判断 (`Cargo.toml` を更新する)
