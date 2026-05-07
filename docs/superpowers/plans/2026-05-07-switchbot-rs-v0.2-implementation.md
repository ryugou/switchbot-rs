# switchbot-rs v0.2 Implementation Plan (mode / status / sync)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** v0.1 にサブコマンド `mode` / `status` / `sync` を追加し、`bump` を「ローカル mode が無ければ API status からモードを infer する」ロジックに更新する。

**Architecture:** 既存の lib + thin bin 構造を維持。新ロジックは `commands.rs` に集約 (`infer_mode_from_status` ヘルパー、`mode_to_str` ヘルパー、`cmd_mode` / `cmd_status` / `cmd_sync` ハンドラ、`cmd_bump` の更新)。`cli.rs` に 3 つの `Command` バリアント追加。`main.rs` で `Mode` / `Status` の stdout 出力を `List` と同じ流儀で扱う。新たな依存追加なし、API スキーマ変更なし。

**Tech Stack:** Rust 1.70+ / clap 4 / reqwest blocking / serde_json / 既存の `BulbStatus` (`api/mod.rs`) と `Mode` (`config.rs`)。

---

## File Structure

```
src/
  cli.rs              # Modify: Command enum に Mode/Status/Sync を追加
  commands.rs         # Modify: 新ハンドラ (cmd_mode/cmd_status/cmd_sync) + ヘルパー (infer_mode_from_status, mode_to_str) + cmd_bump の更新
  main.rs             # Modify: Mode/Status を stdout 出力 + log メッセージ調整
  api/mod.rs          # 変更なし
  config.rs           # 変更なし
  feedback.rs         # 変更なし
  lib.rs              # 変更なし
README.md             # Modify: 新コマンドと smoke test チェックリスト追記
```

新規ファイルなし。既存 4 ファイルへの追記/変更のみ。

---

## 前提

- 設計書: `docs/superpowers/specs/2026-05-07-switchbot-rs-v0.2-design.md`
- v0.1 の現状: `main` ブランチに既存実装あり。テスト 100 件 pass、clippy/fmt clean、`switchbot 0.1.0` がリリース済み
- worktree 推奨パス: `<repo-root>/.worktrees/v0.2-impl` (実装フェーズで `superpowers:using-git-worktrees` 経由で作成)
- 検証コマンドは worktree 内でカレントディレクトリを worktree に置いて実行する想定 (絶対パスは plan に書かない)

---

## Task 1: `infer_mode_from_status` ヘルパー追加

**Files:**
- Modify: `src/commands.rs` (関数追加 + テスト追加)

- [ ] **Step 1: 失敗するテストを追加**

`src/commands.rs` の `mod tests` 内 (既存テストの末尾) に以下を追加:

```rust
    use crate::api::BulbStatus;

    fn make_status(color: &str, color_temperature: u32) -> BulbStatus {
        // BulbStatus は #[derive(Deserialize, Debug)] なので serde_json で組み立てる
        let json = serde_json::json!({
            "power": "on",
            "brightness": 50,
            "color": color,
            "colorTemperature": color_temperature,
        });
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn infer_mode_zero_temperature_is_rgb() {
        let s = make_status("255:128:0", 0);
        assert_eq!(infer_mode_from_status(&s), Mode::Rgb);
    }

    #[test]
    fn infer_mode_positive_temperature_is_temp() {
        let s = make_status("0:0:0", 3000);
        assert_eq!(infer_mode_from_status(&s), Mode::Temp);
    }

    #[test]
    fn infer_mode_zero_color_with_temp_is_temp() {
        // 温度モード時は color が "0:0:0" になる実機挙動
        let s = make_status("0:0:0", 4500);
        assert_eq!(infer_mode_from_status(&s), Mode::Temp);
    }
```

注意: `BulbStatus` の `power` は `Power` enum なので、JSON で `"on"` を渡せば `serde` が `Power::On` にデシリアライズする。`color_temperature` の serde 名は `colorTemperature` (既存の `#[serde(rename = "colorTemperature")]`)。

`use crate::api::BulbStatus;` の重複に注意 (commands.rs の上部に既に `use crate::api::{self, parse_color_str, Client};` がある場合は明示的に `BulbStatus` を取り込む形でも OK)。

- [ ] **Step 2: テスト失敗を確認**

```
cargo test --lib commands::tests::infer_mode 2>&1 | tail -15
```

期待: 「`infer_mode_from_status` not found」相当のコンパイルエラー。

- [ ] **Step 3: 実装を追加**

`src/commands.rs` の上部 (型 `AxisDelta` の定義あたり、`pub fn axis_delta` の近く) に以下を追加:

```rust
/// API status の `colorTemperature` フィールドからモードを推測する。
/// SwitchBot Color Bulb は温度モード時に `color` を "0:0:0" に、
/// RGB モード時に `colorTemperature` を 0 にゼロクリアする (実機検証 2026-05-07)。
pub fn infer_mode_from_status(status: &api::BulbStatus) -> Mode {
    if status.color_temperature == 0 {
        Mode::Rgb
    } else {
        Mode::Temp
    }
}
```

注: `api` と `Mode` は commands.rs の既存 use 宣言 (`use crate::api::{self, parse_color_str, Client};`, `use crate::config::{self, Context, DefaultDevice, DeviceState, Mode};`) に既に含まれている。

- [ ] **Step 4: テスト pass を確認**

```
cargo test --lib commands::tests::infer_mode 2>&1 | tail -10
```

期待: 3 件 pass。

- [ ] **Step 5: 全体ビルド + lint + fmt の clean を確認**

```
cargo build
cargo test --lib 2>&1 | tail -3
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
```

期待: 全 clean、テスト合計 103 件 (v0.1 末尾 100 件 + 新規 3 件) pass。

- [ ] **Step 6: コミット**

```
git add src/commands.rs
git commit -m "feat(commands): add infer_mode_from_status helper for v0.2"
```

---

## Task 2: CLI に `Mode` / `Status` / `Sync` バリアントを追加

**Files:**
- Modify: `src/cli.rs` (Command enum + テスト)

- [ ] **Step 1: 失敗するテストを追加**

`src/cli.rs` の `mod tests` 内 (既存の `on_off_list_parse` テスト直後など) に追加:

```rust
    #[test]
    fn parse_mode_subcommand() {
        let cli = parse(&["mode"]).unwrap();
        assert!(matches!(cli.command, Command::Mode));
    }

    #[test]
    fn parse_status_subcommand() {
        let cli = parse(&["status"]).unwrap();
        assert!(matches!(cli.command, Command::Status));
    }

    #[test]
    fn parse_sync_subcommand() {
        let cli = parse(&["sync"]).unwrap();
        assert!(matches!(cli.command, Command::Sync));
    }
```

`parse` は既存のテストヘルパー (`fn parse(args: &[&str]) -> Result<Cli, clap::Error>`)。

- [ ] **Step 2: テスト失敗を確認**

```
cargo test --lib cli::tests::parse_mode 2>&1 | tail -10
cargo test --lib cli::tests::parse_status 2>&1 | tail -10
cargo test --lib cli::tests::parse_sync 2>&1 | tail -10
```

期待: コンパイルエラー (`Command::Mode` / `Status` / `Sync` 不在)。

- [ ] **Step 3: `Command` enum にバリアント追加**

`src/cli.rs` の `pub enum Command` 末尾 (`List` の後) に以下を追加:

```rust
    Mode,
    Status,
    Sync,
```

変更後の enum 全体は以下のような並び:

```rust
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
    Mode,
    Status,
    Sync,
}
```

- [ ] **Step 4: テスト pass を確認**

```
cargo test --lib cli::tests 2>&1 | tail -3
```

期待: 全 cli テスト pass (新規 3 件含む)。

- [ ] **Step 5: 全体ビルドの clean を確認**

```
cargo build
cargo test --lib 2>&1 | tail -3
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
```

期待: 全 clean。

ただしこの時点では `commands::handle` の match 文に `Mode`/`Status`/`Sync` が無いのでコンパイルエラーになる可能性。**その場合は Task 3-5 が完了するまで `cargo build` が通らないことを許容する**: 既存の `match command { ... }` で `_ => unimplemented!()` を一時追加するか、Task 2 単独では `cargo test --lib cli::tests` のみ確認して、後続タスクで全体ビルドを直す。

**安全策**: Task 2 のここで一時的に `commands.rs` の `handle` 関数末尾に以下を追加して全体ビルドを通す:

```rust
        Command::Mode => Ok(String::new()),    // Task 3 で実装
        Command::Status => Ok(String::new()),  // Task 4 で実装
        Command::Sync => Ok(String::new()),    // Task 5 で実装
```

これは Task 5 完了時に削除する一時 stub。

- [ ] **Step 6: 一時 stub を含めて再度 build + clippy 確認**

```
cargo build
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
```

期待: clean (一時 stub があってもクリッピーは通る)。

- [ ] **Step 7: コミット**

```
git add src/cli.rs src/commands.rs
git commit -m "feat(cli): add Mode/Status/Sync subcommands (handlers stubbed)"
```

---

## Task 3: `cmd_status` 実装

**Files:**
- Modify: `src/commands.rs` (cmd_status + テスト + handle の dispatch 更新)
- Modify: `src/main.rs` (Status の stdout 出力 + log メッセージ)

- [ ] **Step 1: cmd_status 用のテストを追加 (純粋関数の status JSON 整形ヘルパーを切り出す前提)**

JSON 整形は API クライアントを呼ぶ実 I/O とは独立にテストしたいので、まず純粋関数 `format_status_json(status: &BulbStatus) -> Result<String>` を切り出す。

`src/commands.rs` の `mod tests` に以下を追加:

```rust
    #[test]
    fn format_status_json_rgb_mode() {
        let s = make_status("255:128:0", 0);
        let out = format_status_json(&s).unwrap();
        // フィールド順は serde_json::Value で組み立てる順に依存しないので、含有チェック
        assert!(out.contains("\"power\":\"on\""));
        assert!(out.contains("\"brightness\":50"));
        assert!(out.contains("\"color\":\"255:128:0\""));
        assert!(out.contains("\"color_temperature\":0"));
        assert!(out.contains("\"mode\":\"rgb\""));
    }

    #[test]
    fn format_status_json_temp_mode() {
        let s = make_status("0:0:0", 3000);
        let out = format_status_json(&s).unwrap();
        assert!(out.contains("\"color_temperature\":3000"));
        assert!(out.contains("\"mode\":\"temp\""));
    }

    #[test]
    fn format_status_json_is_single_line() {
        let s = make_status("255:0:0", 0);
        let out = format_status_json(&s).unwrap();
        assert!(!out.contains('\n'), "JSON should be single line, got: {}", out);
    }
```

`make_status` は Task 1 で追加済みのヘルパー (既存)。

- [ ] **Step 2: テスト失敗を確認**

```
cargo test --lib commands::tests::format_status_json 2>&1 | tail -10
```

期待: `format_status_json` 未定義のコンパイルエラー。

- [ ] **Step 3: `format_status_json` と `mode_to_str` ヘルパー、`cmd_status` ハンドラを追加**

`src/commands.rs` に以下を追加 (Task 1 の `infer_mode_from_status` の近くに配置):

```rust
/// Mode → 公開ラベル ("rgb" / "temp")。CLI 出力と JSON の両方で使う。
pub fn mode_to_str(mode: Mode) -> &'static str {
    match mode {
        Mode::Rgb => "rgb",
        Mode::Temp => "temp",
    }
}

/// `BulbStatus` を v0.2 設計書通りの JSON 1 行に整形する純粋関数。
fn format_status_json(status: &api::BulbStatus) -> Result<String> {
    let power = match status.power {
        api::Power::On => "on",
        api::Power::Off => "off",
    };
    let mode = mode_to_str(infer_mode_from_status(status));
    let value = serde_json::json!({
        "power": power,
        "brightness": status.brightness,
        "color": status.color,
        "color_temperature": status.color_temperature,
        "mode": mode,
    });
    serde_json::to_string(&value)
        .map_err(|e| anyhow::anyhow!("failed to serialize status JSON: {}", e))
}
```

`cmd_status` ハンドラを `cmd_list` の近くに追加:

```rust
fn cmd_status(client: &Client, device: &DefaultDevice) -> Result<String> {
    let status = client.get_status(&device.id)?;
    format_status_json(&status)
}
```

- [ ] **Step 4: `handle` 関数の `Status` ディスパッチを実装に置換**

`src/commands.rs` の `pub fn handle` 内、Task 2 で追加した一時 stub:

```rust
        Command::Status => Ok(String::new()),  // Task 4 で実装
```

を以下に置換:

```rust
        Command::Status => {
            let device = require_device(ctx)?;
            cmd_status(&client, device)
        }
```

- [ ] **Step 5: テスト pass を確認**

```
cargo test --lib commands::tests::format_status_json 2>&1 | tail -10
```

期待: 3 件 pass。

- [ ] **Step 6: `main.rs` の stdout 処理に `Status` を追加**

`src/main.rs` の `Ok(msg)` ブランチ (`Command::List` を特別扱いしている部分) を以下に変更:

```rust
        Ok(msg) => {
            // list / status は出力を stdout にも流す
            match cli.command {
                cli::Command::List => {
                    use std::io::Write as _;
                    print!("{}", msg);
                    let _ = std::io::stdout().flush();
                    feedback::log_info(&ctx.log_path, "list ok");
                }
                cli::Command::Status => {
                    use std::io::Write as _;
                    println!("{}", msg);
                    let _ = std::io::stdout().flush();
                    feedback::log_info(&ctx.log_path, "status ok");
                }
                _ => {
                    feedback::log_info(&ctx.log_path, &msg);
                }
            }
            Ok(())
        }
```

`use std::io::Write as _;` の重複があるが、ローカル スコープなので問題なし。clippy が警告したら片方を関数冒頭に移動して整理する。

- [ ] **Step 7: 全体ビルドの clean を確認**

```
cargo build
cargo test --lib 2>&1 | tail -3
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
```

期待: 全 clean、テスト 109 件 (v0.1 末尾 100 件 + Task 1 の 3 件 + Task 2 の 3 件 + Task 3 の 3 件) pass。

- [ ] **Step 8: コミット**

```
git add src/commands.rs src/main.rs
git commit -m "feat(status): add status subcommand with JSON output"
```

---

## Task 4: `cmd_mode` 実装

**Files:**
- Modify: `src/commands.rs` (cmd_mode + handle の dispatch 更新)
- Modify: `src/main.rs` (Mode の stdout 出力 + log メッセージ)

- [ ] **Step 1: cmd_mode の挙動を整理**

`cmd_mode` は I/O (`config::read_mode` + `client.get_status`) を扱うため、**ハンドラ自体は単体テストしない**。Task 1 で追加済みの `infer_mode_from_status` と `mode_to_str` (Task 3 で追加) は既にテスト済み。`cmd_mode` の中身はそれらを順に呼ぶだけなので、smoke test で実機検証する。

ユニットテスト追加は無し。

- [ ] **Step 2: `cmd_mode` ハンドラを追加**

`src/commands.rs` の `cmd_status` の隣に追加:

```rust
fn cmd_mode(client: &Client, ctx: &Context, device: &DefaultDevice) -> Result<String> {
    // ローカル mode 優先。なければ API status から infer。
    let mode = match config::read_mode(&ctx.mode_path)? {
        Some(m) => m,
        None => {
            let status = client.get_status(&device.id)?;
            infer_mode_from_status(&status)
        }
    };
    Ok(mode_to_str(mode).to_string())
}
```

- [ ] **Step 3: `handle` 関数の `Mode` ディスパッチを実装に置換**

`src/commands.rs` の `pub fn handle` 内、Task 2 で追加した一時 stub:

```rust
        Command::Mode => Ok(String::new()),    // Task 3 で実装
```

を以下に置換:

```rust
        Command::Mode => {
            let device = require_device(ctx)?;
            cmd_mode(&client, ctx, device)
        }
```

- [ ] **Step 4: `main.rs` の stdout 処理に `Mode` を追加**

Task 3 で更新した main.rs の `match cli.command` に `Mode` ブランチを追加:

```rust
                cli::Command::Mode => {
                    use std::io::Write as _;
                    println!("{}", msg);
                    let _ = std::io::stdout().flush();
                    feedback::log_info(&ctx.log_path, &format!("mode ok ({})", msg));
                }
```

`Status` ブランチの直後または直前に追加。

- [ ] **Step 5: 全体ビルドの clean を確認**

```
cargo build
cargo test --lib 2>&1 | tail -3
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
```

期待: 全 clean、テスト件数は Task 3 と同じ 109 件 (cmd_mode はユニットテスト追加なし)。

- [ ] **Step 6: コミット**

```
git add src/commands.rs src/main.rs
git commit -m "feat(mode): add mode subcommand with API fallback"
```

---

## Task 5: `cmd_sync` 実装

**Files:**
- Modify: `src/commands.rs` (cmd_sync + handle の dispatch 更新)

- [ ] **Step 1: cmd_sync の挙動を整理**

`cmd_sync` も I/O (API + ファイル書き込み) を扱うため単体テスト無し。`infer_mode_from_status` と `config::write_mode` (既存) は別途テスト済み。

- [ ] **Step 2: `cmd_sync` ハンドラを追加**

`src/commands.rs` の `cmd_mode` の隣に追加:

```rust
fn cmd_sync(client: &Client, ctx: &Context, device: &DefaultDevice) -> Result<String> {
    let status = client.get_status(&device.id)?;
    let mode = infer_mode_from_status(&status);
    config::write_mode(&ctx.mode_path, mode)?;
    Ok(format!("sync ok ({})", mode_to_str(mode)))
}
```

- [ ] **Step 3: `handle` 関数の `Sync` ディスパッチを実装に置換**

```rust
        Command::Sync => Ok(String::new()),    // Task 5 で実装
```

を以下に置換:

```rust
        Command::Sync => {
            let device = require_device(ctx)?;
            cmd_sync(&client, ctx, device)
        }
```

`main.rs` 側は `Sync` を特別扱いしない (stdout に何も流さない、log は通常の `&msg` 経路で `"sync ok (rgb)"` 等が記録される)。

- [ ] **Step 4: 全体ビルドの clean を確認**

```
cargo build
cargo test --lib 2>&1 | tail -3
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
```

期待: 全 clean、テスト件数 109 件のまま (cmd_sync はユニットテスト追加なし)。

- [ ] **Step 5: コミット**

```
git add src/commands.rs
git commit -m "feat(sync): add sync subcommand to update local mode from device"
```

---

## Task 6: `cmd_bump` のロジック更新 (ローカル無しで API infer fallback)

**Files:**
- Modify: `src/commands.rs` (cmd_bump 更新 + テスト追加)

- [ ] **Step 1: 失敗するテスト (cmd_bump 内のモード判定挙動) を追加**

`cmd_bump` を直接テストするのは I/O が絡むため難しい。代わりに、ローカル mode の有無に応じた「使うべきモード」を返す純粋ヘルパー `resolve_mode_for_bump` を切り出してテストする。

`src/commands.rs` の `mod tests` に以下を追加:

```rust
    #[test]
    fn resolve_mode_uses_local_when_present() {
        let s = make_status("255:0:0", 3000);
        // ローカルが Rgb なら、status の colorTemperature が 3000 でもローカル優先
        assert_eq!(resolve_mode_for_bump(Some(Mode::Rgb), &s), Mode::Rgb);
    }

    #[test]
    fn resolve_mode_falls_back_to_status_when_local_absent_rgb() {
        let s = make_status("255:0:0", 0);
        assert_eq!(resolve_mode_for_bump(None, &s), Mode::Rgb);
    }

    #[test]
    fn resolve_mode_falls_back_to_status_when_local_absent_temp() {
        let s = make_status("0:0:0", 4000);
        assert_eq!(resolve_mode_for_bump(None, &s), Mode::Temp);
    }
```

- [ ] **Step 2: テスト失敗を確認**

```
cargo test --lib commands::tests::resolve_mode 2>&1 | tail -10
```

期待: `resolve_mode_for_bump` 未定義のコンパイルエラー。

- [ ] **Step 3: `resolve_mode_for_bump` ヘルパーを追加**

`src/commands.rs` の `infer_mode_from_status` の直後に追加:

```rust
/// `bump` 系で使うモードを決定する。ローカル mode 優先、無ければ status から infer。
pub fn resolve_mode_for_bump(local: Option<Mode>, status: &api::BulbStatus) -> Mode {
    local.unwrap_or_else(|| infer_mode_from_status(status))
}
```

- [ ] **Step 4: `cmd_bump` を新ロジックに更新**

現状の `cmd_bump`:

```rust
fn cmd_bump(
    client: &Client,
    ctx: &Context,
    device: &DefaultDevice,
    axis: BumpAxis,
) -> Result<String> {
    let mode = config::read_mode(&ctx.mode_path)?;
    let delta = axis_delta(axis);
    match delta {
        AxisDelta::Red(d) | AxisDelta::Green(d) | AxisDelta::Blue(d) => {
            require_mode(mode, Mode::Rgb)?;
            let status = client.get_status(&device.id)?;
            let (r0, g0, b0) = parse_color_str(&status.color)?;
            let (r, g, b) = match delta {
                AxisDelta::Red(_) => (bump_rgb_channel(r0, d), g0, b0),
                AxisDelta::Green(_) => (r0, bump_rgb_channel(g0, d), b0),
                AxisDelta::Blue(_) => (r0, g0, bump_rgb_channel(b0, d)),
                _ => unreachable!("outer match arm guarantees Red|Green|Blue"),
            };
            client.set_color(&device.id, r, g, b)?;
            Ok(format!("bump {} ok ({}:{}:{})", axis_label(axis), r, g, b))
        }
        AxisDelta::Brightness(d) => {
            let status = client.get_status(&device.id)?;
            let new_value = bump_brightness(status.brightness, d);
            client.set_brightness(&device.id, new_value)?;
            Ok(format!("bump {} ok ({})", axis_label(axis), new_value))
        }
        AxisDelta::Temperature(d) => {
            require_mode(mode, Mode::Temp)?;
            let status = client.get_status(&device.id)?;
            let new_k = bump_temperature(status.color_temperature, d);
            client.set_color_temperature(&device.id, new_k)?;
            Ok(format!("bump {} ok ({}K)", axis_label(axis), new_k))
        }
    }
}
```

を以下に置換:

```rust
fn cmd_bump(
    client: &Client,
    ctx: &Context,
    device: &DefaultDevice,
    axis: BumpAxis,
) -> Result<String> {
    let local_mode = config::read_mode(&ctx.mode_path)?;
    let status = client.get_status(&device.id)?;
    let mode = resolve_mode_for_bump(local_mode, &status);
    let delta = axis_delta(axis);
    match delta {
        AxisDelta::Red(d) | AxisDelta::Green(d) | AxisDelta::Blue(d) => {
            require_mode(Some(mode), Mode::Rgb)?;
            let (r0, g0, b0) = parse_color_str(&status.color)?;
            let (r, g, b) = match delta {
                AxisDelta::Red(_) => (bump_rgb_channel(r0, d), g0, b0),
                AxisDelta::Green(_) => (r0, bump_rgb_channel(g0, d), b0),
                AxisDelta::Blue(_) => (r0, g0, bump_rgb_channel(b0, d)),
                _ => unreachable!("outer match arm guarantees Red|Green|Blue"),
            };
            client.set_color(&device.id, r, g, b)?;
            Ok(format!("bump {} ok ({}:{}:{})", axis_label(axis), r, g, b))
        }
        AxisDelta::Brightness(d) => {
            let new_value = bump_brightness(status.brightness, d);
            client.set_brightness(&device.id, new_value)?;
            Ok(format!("bump {} ok ({})", axis_label(axis), new_value))
        }
        AxisDelta::Temperature(d) => {
            require_mode(Some(mode), Mode::Temp)?;
            let new_k = bump_temperature(status.color_temperature, d);
            client.set_color_temperature(&device.id, new_k)?;
            Ok(format!("bump {} ok ({}K)", axis_label(axis), new_k))
        }
    }
}
```

主な差分:
- `let mode = config::read_mode(...)?;` → `let local_mode = ...;`
- `let status = client.get_status(...)?;` を関数の冒頭に前倒し (3 つの match arm から 1 箇所に集約)
- `let mode = resolve_mode_for_bump(local_mode, &status);` を追加 (mode が常に `Some` 相当)
- `require_mode(mode, Mode::Rgb)` → `require_mode(Some(mode), Mode::Rgb)` (Option ラップを保つ)
- 各 match arm 内の `let status = client.get_status(...)?;` を削除 (前倒し済みなので不要)

- [ ] **Step 5: 既存 require_mode のテストが引き続き pass することを確認**

```
cargo test --lib commands::tests::require_mode 2>&1 | tail -5
cargo test --lib commands::tests::resolve_mode 2>&1 | tail -5
```

期待: require_mode テスト 4 件 (既存) + resolve_mode テスト 3 件 (新規) すべて pass。

- [ ] **Step 6: 全体ビルドの clean を確認**

```
cargo build
cargo test --lib 2>&1 | tail -3
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
```

期待: 全 clean、テスト 112 件 (109 + resolve_mode 3 件) pass。

- [ ] **Step 7: コミット**

```
git add src/commands.rs
git commit -m "feat(bump): use API status to infer mode when local is absent"
```

---

## Task 7: README 更新

**Files:**
- Modify: `README.md` (新コマンド説明 + smoke test 追加)

- [ ] **Step 1: コマンド一覧セクションに 3 つ追加**

`README.md` の `## コマンド` セクション内のコードブロックを更新:

```
switchbot color <hex>            # 例: switchbot color FEDFE1
switchbot bright <1-100|max>     # 例: switchbot bright 50
switchbot temp <2700-6500>       # 例: switchbot temp 3000
switchbot bump <axis>            # 例: switchbot bump R+
switchbot on
switchbot off
switchbot list
switchbot mode                   # 現在のモードを 1 行で出力 (rgb / temp)
switchbot status                 # 電球の現状を JSON で出力
switchbot sync                   # API 状態をローカル mode に反映
```

- [ ] **Step 2: 新コマンドの説明セクションを追加**

`## コマンド` セクションの末尾、`## モード drift について` の直前に以下を追加:

```markdown
### `mode` / `status` / `sync`

`mode` は現在のモードを 1 行で返す:
- ローカル mode ファイル (`~/.switchbot/mode`) があればその値
- 無ければ API `GET /devices/{id}/status` を呼んで `colorTemperature == 0` を判定基準に推測

`status` は電球の現状を JSON で返す (jq 等でパース可能):

```json
{"power":"on","brightness":50,"color":"255:0:0","color_temperature":0,"mode":"rgb"}
```

`sync` は API 状態に合わせてローカル `~/.switchbot/mode` を上書きする。別端末や公式アプリでモードを変更した後の再同期に使う。

なお v0.2 から `bump R/G/B/temp±` は **ローカル mode が無くても動く** (API status の `colorTemperature` から自動推測)。
```

- [ ] **Step 3: smoke test チェックリストに項目を追加**

`## 手動 smoke test (実機チェックリスト)` セクションに以下を追加:

```markdown
- [ ] `switchbot mode` で `rgb` または `temp` が出力される (ローカル mode 有り)
- [ ] `~/.switchbot/mode` を削除 → `switchbot mode` で API ヒューリスティクスから推測される値が出力される
- [ ] `switchbot status` で JSON が出力される (`jq` でパース可能)
- [ ] 公式アプリで電球のモードを変える → `switchbot sync` → `cat ~/.switchbot/mode` で値が更新されている
- [ ] `~/.switchbot/mode` を削除 → `switchbot bump R+` (RGB 状態時) が「モード未設定」エラーで弾かれず動作する
```

- [ ] **Step 4: コミット**

```
git add README.md
git commit -m "docs: update README for v0.2 commands"
```

---

## Task 8: 全体最終確認 + cargo install + 実機 smoke test

**Files:** なし (検証のみ)

- [ ] **Step 1: テスト/lint/fmt/build の最終確認**

```
cargo test --lib 2>&1 | tail -3
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cargo fmt --all -- --check
cargo build --release 2>&1 | tail -3
```

期待: 全 clean、テスト 112 件 pass、release ビルド成功。

- [ ] **Step 2: バイナリインストール**

```
cargo install --path . --root ~/.local --force 2>&1 | tail -3
~/.local/bin/switchbot --version
```

期待: `switchbot 0.2.0` が出る (Cargo.toml の version は別途 bump 不要なら 0.1.0 のままだが、v0.2 機能追加なので bump するのが筋。`Cargo.toml` の `version = "0.1.0"` を `version = "0.2.0"` に上げる Step を Step 3 に挟む)。

- [ ] **Step 3: Cargo.toml のバージョン bump (Step 2 の前にやるのが正式手順)**

`Cargo.toml` を編集:

```toml
[package]
name = "switchbot"
version = "0.2.0"
edition = "2021"
```

```
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to 0.2.0"
```

そして再度 install:

```
cargo install --path . --root ~/.local --force 2>&1 | tail -3
~/.local/bin/switchbot --version
```

期待: `switchbot 0.2.0`。

(Step 2 と 3 の順序を実装時に入れ替えて、bump → install の順で実行する。)

- [ ] **Step 4: 実機 smoke test (新コマンド)**

実機が手元にある前提。

```
~/.local/bin/switchbot mode
# → "rgb" または "temp" のどちらか (現在のモード)

~/.local/bin/switchbot status | jq .
# → JSON 出力。power/brightness/color/color_temperature/mode の 5 フィールド

~/.local/bin/switchbot color FEDFE1
~/.local/bin/switchbot mode
# → "rgb"

~/.local/bin/switchbot temp 3000
~/.local/bin/switchbot mode
# → "temp"

rm ~/.switchbot/mode
~/.local/bin/switchbot mode
# → API ヒューリスティクスから "rgb" or "temp" が出力される

~/.local/bin/switchbot sync
ls ~/.switchbot/mode
cat ~/.switchbot/mode
# → mode ファイルが再生成され、現状に合った値が書かれている

rm ~/.switchbot/mode
~/.local/bin/switchbot bump R+
# → エラーで弾かれず動作する (v0.1 では「モード未設定」エラーだった)
```

すべて期待通り動けば smoke test pass。

- [ ] **Step 5: README の smoke test チェックリストにマークする**

実機確認が pass したら README のチェックボックスを `[x]` にする (任意)。

- [ ] **Step 6: 設計書のレビュー履歴 + Accepted Risk を PR description に追記する**

PR を作成する場合、`code-review` スキルの Step 3 に従って Conventional Commits の commit と Accepted Risk を含む PR description を書く。

---

## 完了基準

- 全 7 タスクのチェックボックスが埋まっている
- `cargo test --lib` が 112 件全 pass
- `cargo clippy --all-targets -- -D warnings` が clean
- `cargo fmt --all -- --check` が clean
- `cargo build --release` が成功
- `~/.local/bin/switchbot --version` が `switchbot 0.2.0`
- 実機 smoke test (Task 8 Step 4) が全項目 pass
- README が新コマンド + smoke test チェックリストを反映済み

---

## 実装中の判断ガイド

- **テストが書きにくい I/O 処理**: 純粋関数 (例: `infer_mode_from_status`, `format_status_json`, `resolve_mode_for_bump`) に切り出してテストする。`cmd_*` ハンドラ自体は単体テストせず手動 smoke test で確認
- **Mode 表記**: ユーザー向けは `"rgb"` / `"temp"` の小文字。`mode_to_str` を必ず経由する
- **新ハンドラのエラーハンドリング**: 既存の `?` パターンで anyhow に伝播。main.rs の通常エラーパス (stderr + log + notify) で処理される
- **clippy の `needless_question_mark` 警告**: もし出たら、Result を直接返す形に書き換える (前回 Copilot レビューで遭遇したパターン)
- **API 伝播ラグ (~5 秒)**: 設計書記載のとおり実害は限定的。テストでは扱わない

---

## 付録: ヘルパー関数の一覧 (Task 完了後の commands.rs)

v0.2 完了時点で commands.rs に存在する pub なヘルパー (テスト対象):

- `clamp` (削除済み、Rust std を使う)
- `bump_rgb_channel` / `bump_brightness` / `bump_temperature` (v0.1 既存)
- `axis_delta` (v0.1 既存)
- `axis_label` (v0.1 既存)
- `require_mode` (v0.1 既存)
- `format_devices_toml` / `sanitize_section_key` / `unique_key` (v0.1 既存)
- **`infer_mode_from_status`** (v0.2 Task 1 で追加)
- **`mode_to_str`** (v0.2 Task 3 で追加)
- **`format_status_json`** (v0.2 Task 3 で追加、private)
- **`resolve_mode_for_bump`** (v0.2 Task 6 で追加)
