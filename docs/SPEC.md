# switchbot-rs 仕様書

SwitchBot Color Bulb を操作する Rust 製シングルバイナリ CLI の初版仕様です。Stream Deck からの呼び出しおよびターミナルからの単体利用を想定しています。

## 概要

- **言語**: Rust
- **ターゲット OS**: macOS（エラー通知に `osascript` を利用するため）
- **対応デバイス**: SwitchBot Color Bulb（W1401400）。他デバイスは初版対象外です。
- **API**: SwitchBot Cloud API v1.1（HMAC-SHA256 署名）
- **配布形態**: シングルバイナリ。`cargo install --path . --root ~/.local` で `~/.local/bin/switchbot` に設置する想定です。

## 利用シーン

1. Stream Deck の Mac Automation プラグイン（"Run Open"）から、`stream-deck-misc` 側に置かれた薄いラッパー `.sh`（例: `sb-color.sh`, `sb-bump.sh`）経由で呼び出されます。
2. ターミナルからも `switchbot color FEDFE1` のように単体で叩けます。

## CLI コマンド

```
switchbot color <hex>             # 例: switchbot color FEDFE1
switchbot bright <0-100|max>      # 例: switchbot bright 50, switchbot bright max
switchbot temp <2700-6500>        # 例: switchbot temp 3000
switchbot bump <axis>             # 例: switchbot bump R+, switchbot bump bright-
switchbot on
switchbot off
switchbot list                    # デバイス一覧（debug 用途）
```

### 引数仕様

| サブコマンド | 引数 | 仕様 |
|---|---|---|
| `color` | `<hex>` | RRGGBB の 16 進 6 桁。`#` なし、大小文字不問。R/G/B 各 0–255 に分解し `setColor "{R}:{G}:{B}"` で送信します。 |
| `bright` | `<N\|max>` | 整数 1–100 または `max`。`max` は内部で 100 として `setBrightness` を送信します。 |
| `temp` | `<K>` | 整数 2700–6500。範囲外はエラーで終了します。 |
| `bump` | `<axis>` | 以下のいずれか: `R+`, `R-`, `G+`, `G-`, `B+`, `B-`, `bright+`, `bright-`。後述の state を読み取り、ステップ幅分だけ加減算した値を送信し、state を更新します。 |
| `on` / `off` | なし | `turnOn` / `turnOff` を `parameter: "default"` で送信します。 |
| `list` | なし | `/v1.1/devices` を呼び、`deviceId` / `deviceName` / `deviceType` を整形して標準出力に表示します。 |

### bump のステップ幅

初版は以下のハードコード値とします。

- RGB（`R±`, `G±`, `B±`）: ±16（0–255 で clamp）
- 明るさ（`bright±`）: ±5（0–100 で clamp）

将来的に `~/.switchbot/config` 等で上書き可能にする余地は残しますが、初版では実装しません。

### 単一デバイス前提

初版は「`devices` ファイルの最初のエントリ（または名前 `default` のエントリ）」を対象として動作します。`--device <name>` 引数は将来拡張として実装しません。

## ファイル構成（リポジトリ）

```
switchbot-rs/
  Cargo.toml
  src/
    main.rs       # CLI エントリポイント、clap 定義
    cli.rs        # サブコマンド構造体（clap derive）
    api.rs        # SwitchBot API 呼び出し（HTTP + JSON）
    signing.rs    # HMAC-SHA256 署名生成
    config.rs     # secrets / devices ファイル読み込み
    state.rs      # state ファイル読み書き
    notify.rs     # macOS 通知（osascript）
    log.rs        # ログ出力
  README.md
  SPEC.md         # 本ファイル
  .gitignore      # target/ 等
```

モジュール分割は目安です。`main.rs` の肥大化を避けつつ、責務ごとに小さく切ってください。

## 設定・状態ファイル（実行時、リポ外）

すべて `~/.switchbot/` 配下に配置し、フォーマットは TOML で統一します。

### `~/.switchbot/secrets`（必須・手動編集）

```toml
token = "..."
secret = "..."
```

- 権限は `chmod 600` を推奨します（バイナリ側からは強制しないが、初回生成時は 600 で書き出します）。

### `~/.switchbot/devices`（必須・手動編集または `list` 結果から作成）

```toml
[default]
id = "01-202311241234-12345678"
type = "color-bulb"
```

- 初版は `default` セクションのみ参照します。

### `~/.switchbot/state`（自動生成・自動更新）

```toml
r = 255
g = 128
b = 0
brightness = 50
```

- `color` / `bright` / `temp` / `bump` の成功時に更新します。
- 初回はファイルがないので、bump 系コマンドが先に呼ばれた場合は「デフォルト値（例: r=255, g=255, b=255, brightness=100）」から開始します。

### `~/.switchbot/log`（自動生成・append-only）

```
2026-05-04T12:34:56+09:00 INFO  color FEDFE1 ok
2026-05-04T12:35:01+09:00 ERROR bump R+ failed: HTTP 500 …
```

- フォーマットは「ISO8601 タイムスタンプ + レベル + 1 行メッセージ」とします。
- 初版はローテーション・サイズ制限なし。手動 truncate を許容します。

## 初回実行時の挙動

1. `~/.switchbot/` ディレクトリがなければ作成します。
2. `secrets` がなければ空の雛形を `chmod 600` で書き出し、以下のメッセージを stderr に出力して exit code 1 で終了します。
   ```
   ~/.switchbot/secrets を編集して token と secret を設定してください。
   ```
3. `devices` がなければ空の雛形を書き出し、以下のメッセージで終了します。
   ```
   ~/.switchbot/devices にデバイスを登録してください。switchbot list で deviceId を確認できます。
   ```
4. `state` がない状態で `bump` 系を呼ばれた場合、デフォルト値で開始し state を新規作成します。

## 状態管理

- `color` / `bright` / `temp` 成功時に state の対応フィールドを更新します。
- `bump` は state を読み込み、対象フィールドにステップ幅を加減算（clamp 含む）した上で対応する API コマンド（`setColor` または `setBrightness`）を送信し、成功したら state を更新します。
- 並列実行は想定しないためファイルロックは不要です。

## エラー時のフィードバック

Stream Deck から呼ばれた場合、stderr / stdout はユーザーから見えません。失敗時は以下を行います。

1. **stderr** へエラーメッセージを 1 行で出力（ターミナル単体利用時の利便性）。
2. **macOS 通知**: `osascript -e 'display notification "<msg>" with title "switchbot"'` を実行します。
3. **ログファイル**: `~/.switchbot/log` に `ERROR …` で 1 行追記します。

成功時は無音とします（ログには `INFO …` を残す）。

## SwitchBot API v1.1 実装メモ

### エンドポイント

- Base URL: `https://api.switch-bot.com`
- デバイス一覧: `GET /v1.1/devices`
- コマンド送信: `POST /v1.1/devices/{deviceId}/commands`

### リクエストヘッダ（毎回必須）

| ヘッダ | 値 |
|---|---|
| `Authorization` | アプリ取得済みの token |
| `sign` | HMAC-SHA256(`token + t + nonce`, `secret`) を base64 化して大文字化 |
| `t` | 13 桁のミリ秒タイムスタンプ |
| `nonce` | リクエストごとにランダムな UUID |
| `Content-Type` | `application/json` |

### リクエストボディ

```json
{ "command": "...", "parameter": "...", "commandType": "command" }
```

- `commandType` は `"command"` 固定です。

### レスポンス

- `statusCode == 100` が成功です。
- それ以外は `body.message` にエラー理由が入ります。エラー時はその内容を通知 / ログに含めてください。

### Color Bulb コマンドマッピング

| 内部サブコマンド | API command | parameter |
|---|---|---|
| `on` | `turnOn` | `"default"` |
| `off` | `turnOff` | `"default"` |
| `color FEDFE1` | `setColor` | `"254:223:225"`（10 進 R:G:B） |
| `bright 50` | `setBrightness` | `"50"` |
| `bright max` | `setBrightness` | `"100"` |
| `temp 3000` | `setColorTemperature` | `"3000"` |

### レート制限

- 1 トークンあたり 10,000 calls/day。超過時は 401 が返ります。初版では特別なハンドリングは不要ですが、ログには 401 の発生を明示してください。

## クレート依存

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
reqwest = { version = "0.12", features = ["rustls-tls", "blocking", "json"], default-features = false }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
hmac = "0.12"
sha2 = "0.10"
base64 = "0.22"
uuid = { version = "1", features = ["v4"] }
directories = "5"
anyhow = "1"
chrono = "0.4"   # ログのタイムスタンプ用
```

- `reqwest` は **rustls-tls** + **blocking** を採用し、OpenSSL 依存を排除して macOS でのビルドを単純化します。
- 非同期ランタイム（`tokio` 等）は不要です。

## ビルド・インストール

```bash
# リリースビルドのみ
cargo build --release

# ~/.local/bin/switchbot にインストール
cargo install --path . --root ~/.local
```

- `~/.local/bin` が PATH に通っていることを README で前提にします。
- Stream Deck から呼ばれる側（`stream-deck-misc` の `.sh`）はフルパス `$HOME/.local/bin/switchbot` を直接書く運用です（Stream Deck 起動時の PATH が貧弱なため）。

## 動作環境

- macOS（Apple Silicon / Intel いずれも）
- Rust 1.70 以上
- SwitchBot アプリでクラウドサービスを有効化済みの Color Bulb

## 初版に含めない項目

以下は将来拡張とし、初版では実装しません。

- 複数デバイス対応（`--device <name>` 引数、`devices` の複数セクション切り替え）
- bump ステップ幅の設定可能化（`~/.switchbot/config` 等）
- ログのローテーション・サイズ制限
- インタラクティブな初期セットアップコマンド（`switchbot configure`）
- Color Bulb 以外のデバイスタイプ
- 非同期 / 並列リクエスト

## テスト方針（推奨）

- **ユニットテスト**: `signing.rs`（HMAC 署名の既知ベクトル検証）、`state.rs`（TOML の round-trip）、bump の clamp ロジック
- **統合テスト**: API 呼び出し部分は `wiremock` 等でモック化、または手動の smoke test
- 実機を使った手動確認のチェックリストは README 側で別途定義してください
