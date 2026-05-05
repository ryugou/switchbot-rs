# Copilot レビュー指摘 9 件反映 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** switchbot-rs v2-impl に Copilot レビュー指摘 9 件を反映し、cargo test/fmt/clippy/build がすべて clean であることを確認する。

**Architecture:** 修正はコア 3 ファイル (`main.rs`, `api/mod.rs`, `config.rs`) とドキュメント 2 件 (`README.md`, 設計書)。各修正は独立しており依存関係なし。テストは TDD 順で追加。

**Tech Stack:** Rust 2021、clap 4 (derive)、reqwest 0.12 (blocking)、anyhow 1、serde_json 1

---

## Task 1: 修正1+3 — `main.rs` を `try_parse()` ベースに書き換え + stdout flush

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: 現状確認**

```bash
cargo test --lib 2>&1 | tail -5
# Expected: test result: ok. 84 passed
```

- [ ] **Step 2: `src/main.rs` を全書き換え**

```rust
use clap::Parser;

use switchbot::{cli, commands, config, feedback};

fn main() {
    let exit_code = match run() {
        Ok(()) => 0,
        Err(()) => 1,
    };
    std::process::exit(exit_code);
}

/// 戻り値の Err は単に「失敗した」を意味する。詳細メッセージは feedback で出力済み。
fn run() -> Result<(), ()> {
    // 1) 引数パース前にロードできる範囲で log_path だけ取得しておく
    //    (引数バリデーション失敗時にも log/notify を出すため)
    let log_path = config::config_dir().ok().map(|d| d.join("log"));

    // 2) 引数パース。失敗時は help/version は通常通り、それ以外は feedback 経由で通知。
    let cli = match cli::Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            // help/version は exit 0 で stdout に出す (notify しない)
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) {
                e.print().ok();
                return Ok(());
            }
            // 引数バリデーション失敗: stderr + log + notify
            let msg = e.to_string();
            if let Some(ref lp) = log_path {
                feedback::log_error(lp, &msg);
            }
            feedback::notify(&msg);
            eprintln!("{}", msg);
            return Err(());
        }
    };

    // 3) Context をロード。失敗 (HOME 不在、初回 bootstrap、op inject 失敗等) は stderr のみ。
    let ctx = match config::load_context() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            return Err(());
        }
    };

    // 4) コマンドを実行。成功時はログ INFO、失敗時はログ ERROR + 通知 + stderr。
    match commands::handle(&cli.command, &ctx) {
        Ok(msg) => {
            feedback::log_info(&ctx.log_path, &msg);
            // list は出力を stdout にも流す
            if let cli::Command::List = cli.command {
                use std::io::Write as _;
                print!("{}", msg);
                let _ = std::io::stdout().flush();
            }
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

- [ ] **Step 3: ビルド確認**

```bash
cd /Users/ryugo/Developer/src/personal/switchbot-rs/.worktrees/v2-impl && cargo build 2>&1
# Expected: Compiling switchbot ... Finished
```

- [ ] **Step 4: テスト確認 (件数維持)**

```bash
cd /Users/ryugo/Developer/src/personal/switchbot-rs/.worktrees/v2-impl && cargo test --lib 2>&1 | tail -5
# Expected: test result: ok. 84 passed
```

---

## Task 2: 修正6+7+8 — `api/mod.rs` の `error_for_status()` 廃止 + `parse_response_inner` テスト追加

**Files:**
- Modify: `src/api/mod.rs`

- [ ] **Step 1: `parse_response_inner` のテストを先に書く (TDD)**

`src/api/mod.rs` の `#[cfg(test)]` ブロック末尾に以下を追加:

```rust
    #[test]
    fn parse_response_inner_includes_api_message_on_http_error() {
        let body = r#"{"statusCode": 401, "message": "unauthorized", "body": null}"#;
        let result: Result<ApiResponse<DeviceList>> =
            parse_response_inner(body, reqwest::StatusCode::UNAUTHORIZED, "list_devices");
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("HTTP 401"), "msg={}", msg);
        assert!(msg.contains("unauthorized"), "msg={}", msg);
    }

    #[test]
    fn parse_response_inner_falls_back_to_raw_body_on_non_json_error() {
        let body = "Internal Server Error";
        let result: Result<ApiResponse<DeviceList>> =
            parse_response_inner(body, reqwest::StatusCode::INTERNAL_SERVER_ERROR, "list_devices");
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("HTTP 500"), "msg={}", msg);
        assert!(msg.contains("Internal Server Error"), "msg={}", msg);
    }

    #[test]
    fn parse_response_inner_returns_ok_on_success() {
        let body =
            r#"{"statusCode": 100, "message": "success", "body": {"deviceList": []}}"#;
        let result: Result<ApiResponse<DeviceList>> =
            parse_response_inner(body, reqwest::StatusCode::OK, "list_devices");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().status_code, 100);
    }
```

- [ ] **Step 2: テストが失敗することを確認 (関数がまだない)**

```bash
cd /Users/ryugo/Developer/src/personal/switchbot-rs/.worktrees/v2-impl && cargo test --lib api 2>&1 | tail -10
# Expected: error[E0425]: cannot find function `parse_response_inner`
```

- [ ] **Step 3: `parse_response_inner` と `parse_response` の実装を追加し、各メソッドを書き直す**

`Client` の `impl` ブロック内 (`turn_off` の後など) に以下を追加:

```rust
    /// レスポンスから `ApiResponse<T>` をパースする。
    /// HTTP 4xx/5xx でも、body が SwitchBot API の JSON 形式なら message をエラーに含める。
    fn parse_response<T: serde::de::DeserializeOwned>(
        resp: reqwest::blocking::Response,
        op: &str,
    ) -> Result<ApiResponse<T>> {
        let status = resp.status();
        let body_text = resp
            .text()
            .with_context(|| format!("failed to read response body ({})", op))?;
        parse_response_inner::<T>(&body_text, status, op)
    }
```

`impl Client` の外 (ただし `#[cfg(test)]` の前) に free 関数として:

```rust
fn parse_response_inner<T: serde::de::DeserializeOwned>(
    body_text: &str,
    status: reqwest::StatusCode,
    op: &str,
) -> Result<ApiResponse<T>> {
    match serde_json::from_str::<ApiResponse<T>>(body_text) {
        Ok(api) => {
            if !status.is_success() && api.status_code != 100 {
                return Err(anyhow::anyhow!(
                    "{} HTTP {}: {} (statusCode={})",
                    op,
                    status.as_u16(),
                    api.message,
                    api.status_code
                ));
            }
            Ok(api)
        }
        Err(_) if !status.is_success() => {
            let snippet = body_text
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(200)
                .collect::<String>();
            Err(anyhow::anyhow!(
                "{} HTTP {}: {}",
                op,
                status.as_u16(),
                snippet
            ))
        }
        Err(e) => Err(anyhow::anyhow!("failed to decode {} JSON: {}", op, e)),
    }
}
```

`list_devices` を書き直し:

```rust
    pub fn list_devices(&self) -> Result<Vec<Device>> {
        let url = format!("{}/v1.1/devices", BASE_URL);
        let resp = self
            .http
            .get(&url)
            .headers(self.auth_headers()?)
            .send()
            .context("HTTP request failed (list_devices)")?;
        let api: ApiResponse<DeviceList> = Self::parse_response(resp, "list_devices")?;
        check_status(&api)?;
        Ok(api.body.context("empty body in list_devices")?.device_list)
    }
```

`get_status` を書き直し:

```rust
    pub fn get_status(&self, device_id: &str) -> Result<BulbStatus> {
        let url = format!("{}/v1.1/devices/{}/status", BASE_URL, device_id);
        let resp = self
            .http
            .get(&url)
            .headers(self.auth_headers()?)
            .send()
            .context("HTTP request failed (get_status)")?;
        let api: ApiResponse<BulbStatus> = Self::parse_response(resp, "get_status")?;
        check_status(&api)?;
        api.body.context("empty body in get_status")
    }
```

`send_command` を書き直し:

```rust
    fn send_command(&self, device_id: &str, command: &str, parameter: &str) -> Result<()> {
        let url = format!("{}/v1.1/devices/{}/commands", BASE_URL, device_id);
        let body = serde_json::json!({
            "command": command,
            "parameter": parameter,
            "commandType": "command",
        });
        let resp = self
            .http
            .post(&url)
            .headers(self.auth_headers()?)
            .json(&body)
            .send()
            .context("HTTP request failed (send_command)")?;
        let api: ApiResponse<serde_json::Value> = Self::parse_response(resp, "send_command")?;
        check_status(&api)?;
        Ok(())
    }
```

- [ ] **Step 4: テストが通ることを確認**

```bash
cd /Users/ryugo/Developer/src/personal/switchbot-rs/.worktrees/v2-impl && cargo test --lib api 2>&1 | tail -15
# Expected: test result: ok. N passed; 0 failed
# parse_response_inner_* 3 テストが全て ok
```

- [ ] **Step 5: fmt/clippy 確認**

```bash
cd /Users/ryugo/Developer/src/personal/switchbot-rs/.worktrees/v2-impl && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings 2>&1 | tail -10
# Expected: no errors
```

---

## Task 3: 修正9 — `config.rs` の `[default].id` whitespace チェック + テスト追加

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: テストを先に書く (TDD)**

`src/config.rs` の `#[cfg(test)]` ブロック末尾に追加:

```rust
    #[test]
    fn load_devices_whitespace_only_id_returns_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        fs::write(
            &path,
            "[default]\nid = \"   \"\ntype = \"Color Bulb\"\n",
        )
        .unwrap();
        let result = load_devices(&path).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn load_devices_id_is_trimmed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("devices");
        fs::write(
            &path,
            "[default]\nid = \"  abc  \"\n",
        )
        .unwrap();
        let result = load_devices(&path).unwrap().unwrap();
        assert_eq!(result.id, "abc");
    }
```

- [ ] **Step 2: テストが失敗することを確認**

```bash
cd /Users/ryugo/Developer/src/personal/switchbot-rs/.worktrees/v2-impl && cargo test --lib config::tests::load_devices_whitespace 2>&1 | tail -10
# Expected: FAILED (whitespace_only は None を返さず Some を返す)
```

- [ ] **Step 3: `load_devices` の `if device.id.is_empty()` を trim チェックに変更**

`src/config.rs` の `load_devices` 関数内:

```rust
    // 変更前:
    let device = match parsed.default {
        Some(d) => d,
        None => return Ok(None),
    };
    if device.id.is_empty() {
        return Ok(None);
    }
    Ok(Some(device))
```

```rust
    // 変更後:
    let mut device = match parsed.default {
        Some(d) => d,
        None => return Ok(None),
    };
    device.id = device.id.trim().to_string();
    if device.id.is_empty() {
        return Ok(None);
    }
    Ok(Some(device))
```

- [ ] **Step 4: テストが通ることを確認**

```bash
cd /Users/ryugo/Developer/src/personal/switchbot-rs/.worktrees/v2-impl && cargo test --lib config 2>&1 | tail -10
# Expected: test result: ok. N passed (load_devices_whitespace_only_id_returns_none と load_devices_id_is_trimmed が ok)
```

---

## Task 4: 修正2 — `README.md` バイナリサイズ表記修正

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 該当行を編集**

`README.md` の:
```
- 単一バイナリ (~2-3 MB)。`~/.local/bin/switchbot` に置くだけ
```
を:
```
- 単一バイナリ (~4-5 MB)。`~/.local/bin/switchbot` に置くだけ
```
に変更。

---

## Task 5: 修正4+5 — 設計書の bootstrap フロー + bright 表記更新

**Files:**
- Modify: `docs/superpowers/specs/2026-05-04-switchbot-rs-design.md`

- [ ] **Step 1: 修正5 — `bright <0-100|max>` → `bright <1-100|max>`**

設計書 CLI 仕様セクションの:
```
switchbot bright <0-100|max>      # 例: switchbot bright 50, switchbot bright max
```
を:
```
switchbot bright <1-100|max>      # 例: switchbot bright 50, switchbot bright max
```
に変更。

- [ ] **Step 2: 修正4 — 初回起動時の挙動セクション item 3 を更新**

設計書「### 初回起動時の挙動」の item 3:
```
3. `devices` がなければ「~/.switchbot/devices」セクションのテンプレを書き出して exit 1。stderr: `switchbot list で deviceId を確認できます`
```
を:
```
3. `devices` がなければ「~/.switchbot/devices」セクションのテンプレを書き出すが、`switchbot list` 経路だけは続行可能 (`device = None` で進む)。それ以外のコマンドは「デバイスが未設定です」エラーで exit 1。stderr: `switchbot list で deviceId を確認できます`
```
に変更。

---

## Task 6: 全体検証

**Files:** なし (読み取り専用)

- [ ] **Step 1: `cargo test --lib`**

```bash
cd /Users/ryugo/Developer/src/personal/switchbot-rs/.worktrees/v2-impl && cargo test --lib 2>&1 | tail -5
# Expected: test result: ok. 90 passed (84 + 5 new = 89 以上)
```

- [ ] **Step 2: `cargo fmt --all -- --check`**

```bash
cd /Users/ryugo/Developer/src/personal/switchbot-rs/.worktrees/v2-impl && cargo fmt --all -- --check 2>&1
# Expected: (no output = clean)
```

- [ ] **Step 3: `cargo clippy --all-targets -- -D warnings`**

```bash
cd /Users/ryugo/Developer/src/personal/switchbot-rs/.worktrees/v2-impl && cargo clippy --all-targets -- -D warnings 2>&1 | tail -10
# Expected: warning: ... 0 errors
```

- [ ] **Step 4: `cargo build --release`**

```bash
cd /Users/ryugo/Developer/src/personal/switchbot-rs/.worktrees/v2-impl && cargo build --release 2>&1 | tail -5
# Expected: Finished release [optimized] target(s)
```

---

## Task 7: コミット

- [ ] **Step 1: ステージング**

```bash
git add src/main.rs src/api/mod.rs src/config.rs README.md docs/superpowers/specs/2026-05-04-switchbot-rs-design.md
```

- [ ] **Step 2: コミット**

```bash
git commit -m "fix: Copilot レビュー指摘 9 件を反映

- main: try_parse で引数エラーも feedback (notify/log) 経路に流す
- main: list 出力で stdout を明示 flush して切捨て防止
- api: error_for_status() を廃し、4xx/5xx でも JSON body の message を抽出
- config: [default].id を trim してチェック (whitespace のみを未設定扱いに)
- README: バイナリサイズを実測 4-5 MB に修正
- docs: 設計書の bootstrap フロー (devices 不在で list 続行) と bright 範囲表記を更新"
```
