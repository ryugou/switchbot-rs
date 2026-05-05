# switchbot-rs 設計書 (v2)

> 本ドキュメントは `docs/SPEC.md` (v1) のブレストを経て改訂された設計です。`docs/SPEC.md` との差分は末尾「SPEC.md (v1) からの主な変更点」を参照してください。実装はこちらを正典とします。

## 背景・目的

SwitchBot Color Bulb (W1401400) を Stream Deck から快適に操作するための Rust 製シングルバイナリ CLI。ターミナルからの単体利用も想定。

ブレストで「公式ツールとして筋が通る設計」を優先する方針が選ばれ、v1 仕様にあった以下の弱点を是正している:

- ローカル `state` ファイルが SwitchBot アプリ等の外部操作で容易にズレる
- 温度モード中に `bump R+` を実行すると古い RGB 値で勝手にモード切替が起こる
- 認証情報をプレーンテキストで `~/.switchbot/secrets` に保持する

## CLI 仕様

```
switchbot color <hex>             # 例: switchbot color FEDFE1   (RGB モードへ)
switchbot bright <1-100|max>      # 例: switchbot bright 50, switchbot bright max
switchbot temp <2700-6500>        # 例: switchbot temp 3000      (温度モードへ)
switchbot bump <axis>             # 例: switchbot bump R+, switchbot bump temp-
switchbot on
switchbot off
switchbot list                    # デバイス一覧 (TOML 形式)
```

### 引数仕様

| サブコマンド | 引数 | 仕様 |
|---|---|---|
| `color` | `<hex>` | RRGGBB の 16 進 6 桁。`#` なし、大小文字不問。R/G/B 各 0–255 に分解し `setColor "{R}:{G}:{B}"` で送信。成功時にモードファイルを `mode = "rgb"` に更新。 |
| `bright` | `<N\|max>` | 整数 1–100 または `max` (=100)。`setBrightness` を送信。モードファイルは変更しない。`0` は `setBrightness` API が受け付けないため不可。消灯は `switchbot off` を使う。 |
| `temp` | `<K>` | 整数 2700–6500。範囲外はバリデーションエラー。`setColorTemperature` を送信。成功時にモードファイルを `mode = "temp"` に更新。 |
| `bump` | `<axis>` | 後述の 10 axis のいずれか。GET status で現在値を取得 → ステップ幅を加減算 (clamp) → 対応コマンドで送信。 |
| `on` / `off` | なし | `turnOn` / `turnOff` を `parameter: "default"` で送信。 |
| `list` | なし | `/v1.1/devices` を呼び、TOML 形式で標準出力。`switchbot list > ~/.switchbot/devices` で初回設定が完結する想定 (出力フォーマットは「list の TOML 出力」セクション参照)。 |

### bump の axis (10 種) と step 幅

| axis | step | clamp 範囲 | 許可されるモード |
|---|---|---|---|
| `R+` `R-` | ±16 | 0–255 | RGB モードのみ |
| `G+` `G-` | ±16 | 0–255 | RGB モードのみ |
| `B+` `B-` | ±16 | 0–255 | RGB モードのみ |
| `bright+` `bright-` | ±10 | 1–100 | 両モード可 |
| `temp+` `temp-` | ±100 | 2700–6500 | 温度モードのみ |

step 幅はハードコード。`~/.switchbot/config` 等での上書きは初版では実装しない。

### モード不一致時の挙動

`bump R/G/B±` を温度モード時に、または `bump temp±` を RGB モード時に実行した場合は exit 1 + 通知:

```
# 例: temp モード中に bump R+
error: 現在 温度モードです。switchbot color <hex> を先に実行してください。

# 例: RGB モード中に bump temp+
error: 現在 RGB モードです。switchbot temp <K> を先に実行してください。
```

`bump bright±` は両モードで有効。モードファイル未作成 (初回起動) の状態で `bump R/G/B/temp±` が呼ばれた場合は「モード未設定」エラーで exit 1。`bump bright±` はモードファイルを参照しないので初期化前でも動作する。

### 単一デバイス前提

`devices` ファイルの `[default]` セクションのみ参照。`--device <name>` 引数は将来拡張。

### list の TOML 出力

`/v1.1/devices` のレスポンスから取得したデバイスを TOML として標準出力に書き出す。各デバイスは独立したセクションになる。

| TOML フィールド | 値の出所 |
|---|---|
| セクション名 | デバイスが 1 台だけなら `default` 固定。複数台なら `deviceName` を kebab-case (lower、英数字とハイフンのみ、それ以外は `-` に置換) に正規化 |
| `id` | API の `deviceId` |
| `type` | API の `deviceType` をそのまま (例: `"Color Bulb"`) |
| `name` | API の `deviceName` (元の表示名) |

```toml
# デバイスが 1 台のとき
[default]
id = "01-202311241234-12345678"
type = "Color Bulb"
name = "Living Bulb"

# 複数台のとき (ユーザーが手で 1 つを [default] にリネームする想定)
[living-bulb]
id = "01-202311241234-12345678"
type = "Color Bulb"
name = "Living Bulb"

[bedroom-plug]
id = "02-202311241234-87654321"
type = "Plug Mini"
name = "Bedroom Plug"
```

## アーキテクチャ

### モジュール構成

```
src/
  main.rs              # エントリポイント。clap parse → dispatch → feedback。
  cli.rs               # clap derive (Cli, Command, BumpAxis)。
  config.rs            # .env (1Password 解決) / devices / mode の読み書き。
  feedback.rs          # ログ出力 + osascript 通知。
  commands.rs          # サブコマンドハンドラ (cmd_color, cmd_bump, …)。
  api/
    mod.rs             # 公開関数 (list_devices, get_status, set_color, ...)。
    signing.rs         # private: HMAC-SHA256 署名生成。
```

`signing` を `api/` 配下に置くのは、署名生成が API 呼び出し以外で使われない実装詳細であり、トップレベルに並べると「他用途もあるのか」と読み手を迷わせるため。`api::signing` という命名で API 専用と明示する。

`commands/` をディレクトリ分割しないのは、bump を含めても合計 200 行程度に収まる見積もりのため。

### 典型データフロー (`switchbot bump R+`)

```
main.rs
  └─ cli::Cli::parse() → Command::Bump(BumpAxis::RPlus)
  └─ config::load() → Config { token, secret, device_id }
  └─ commands::bump(BumpAxis::RPlus, &config)
       ├─ config::read_mode()                  → Mode::Rgb (or error if no mode file)
       ├─ require Mode::Rgb (else error)
       ├─ api::get_status(&config)             → Status { color: "100:128:0", ... }
       ├─ clamp(100 + 16, 0, 255) = 116
       └─ api::set_color(&config, 116, 128, 0)
  └─ feedback::report(result)
       ├─ Ok  → log INFO  "bump R+ ok"
       └─ Err → log ERROR + osascript notify + stderr 出力
  └─ exit 0 or 1
```

### エラー型

`anyhow::Result` で統一。最外殻 (`main.rs`) で `Result` を `feedback::report` に渡し、そこで分岐する。`thiserror` で型を切る価値が薄いので採用しない (1 箇所でしか分岐しない、エラーは人間向けメッセージで十分)。

### Exit code

- `0`: 成功
- `1`: 失敗全般 (種類で分岐させない。Stream Deck は exit code を区別しないため意味がない)

## 設定とファイル

すべて `~/.switchbot/` 配下。

### `~/.switchbot/.env` (必須・手動編集)

認証情報の置き場。1Password の secret reference (`op://<vault>/<item>/<field>`) と素のリテラル値が混在可能。

```
# 1Password 連携 (推奨)
SWITCHBOT_TOKEN=op://ai-agents/switchbot_api_token/credential
SWITCHBOT_SECRET=op://ai-agents/switchbot_client_secret/credential

# テスト用途で直接値を書く場合
# SWITCHBOT_TOKEN=...
# SWITCHBOT_SECRET=...
```

#### 解決ロジック

起動時に `.env` を読み、`op://` で始まる値が **1 つでもあれば** `op inject -i ~/.switchbot/.env` を 1 回 shell out して解決済みテキストを stdout から取得し、自前でパースしてメモリ上の Config に格納する。

`op://` が **1 つも無ければ** op を起動せず、`.env` を直接パースして使う。これによりテスト/CI/Linux ヘッドレス環境で `op` 未導入でも平文値だけで動作させられる。

#### エラー条件

- `op://` が `.env` 内に存在するが `op` CLI が PATH にない → 通知 + 「1Password CLI (`op`) が必要です」
- `op inject` が non-zero 終了 → 通知 + 「1Password の解決に失敗しました (unlock されていますか?)」
- 解決後に `SWITCHBOT_TOKEN` か `SWITCHBOT_SECRET` が空 → 通知 + 「.env の値が空です」

1Password の biometric unlock を有効化しておくことで Touch ID プロンプトを最小化する (1Password アプリ側の設定)。

### `~/.switchbot/devices` (必須・手動編集または `list` 結果から作成)

```toml
[default]
id = "01-202311241234-12345678"
type = "Color Bulb"
name = "Living Bulb"
```

初版は `[default]` セクションの `id` のみ実装上必須。`type` `name` は将来の複数デバイス対応や表示用。

初回起動時に書き出されるテンプレ:

```toml
# ~/.switchbot/devices
# `switchbot list` の出力をリダイレクトするか、手書きで埋めてください。
[default]
id = ""
type = "Color Bulb"
```

### `~/.switchbot/mode` (自動生成・自動更新)

```toml
mode = "rgb"   # または "temp"
```

`color` 成功時に `"rgb"`, `temp` 成功時に `"temp"` を書き込む。`bump R/G/B/temp±` 実行時に読み込み、対象 axis と一致しなければエラー。

このファイルは「**現在のモード 1 ビット**」だけを保持する最小限の状態。値 (RGB / 温度 / 明るさ) はすべて GET status から取るので、ローカルとデバイスがズレて困る情報量はモードビットだけに限定される。

#### モード drift について

別端末や公式アプリでモードが変えられた場合、本 CLI のモードファイルは古い値のまま残る。`bump R/G/B/temp±` がモード不一致エラーで弾かれたら、`switchbot color <hex>` か `switchbot temp <K>` を一度実行して再同期する。これは仕様上の制約であり、実害は通知 1 回 + ユーザーの 1 アクションで済む範囲。

### `~/.switchbot/log` (自動生成・append-only)

```
2026-05-04T12:34:56+09:00 INFO  color FEDFE1 ok
2026-05-04T12:35:01+09:00 ERROR bump R+ failed: 現在温度モードです
```

ISO 8601 タイムスタンプ (JST) + レベル (`INFO` / `ERROR`) + 1 行メッセージ。ローテーション・サイズ制限なし。手動 truncate を許容。

### 初回起動時の挙動

1. `~/.switchbot/` がなければ作成。
2. `.env` がなければ以下のテンプレを書き出して exit 1:
   ```
   # 1Password 連携 (推奨):
   SWITCHBOT_TOKEN=op://Personal/SwitchBot/token
   SWITCHBOT_SECRET=op://Personal/SwitchBot/secret
   # 直接値を書く場合 (テスト用途等):
   # SWITCHBOT_TOKEN=...
   # SWITCHBOT_SECRET=...
   ```
   stderr: `~/.switchbot/.env を編集してください`
3. `devices` がなければ「~/.switchbot/devices」セクションのテンプレを書き出すが、`switchbot list` 経路だけは続行可能 (`device = None` で進む)。それ以外のコマンドは「デバイスが未設定です」エラーで exit 1。stderr: `switchbot list で deviceId を確認できます`
4. `mode` がなくても起動は通る (bump R/G/B/temp 系はエラーになるが、color/temp/bright/on/off/list は問題なく動く)。
5. `directories::BaseDirs::new()` が `None` を返す環境 (`HOME` 未設定など、macOS では稀) は exit 1。stderr: `ホームディレクトリを特定できません`

## API 詳細

### ベース URL とエンドポイント

- Base: `https://api.switch-bot.com`
- デバイス一覧: `GET /v1.1/devices`
- ステータス取得: `GET /v1.1/devices/{deviceId}/status`
- コマンド送信: `POST /v1.1/devices/{deviceId}/commands`

### 認証ヘッダ (毎リクエスト必須)

| ヘッダ | 値 |
|---|---|
| `Authorization` | `SWITCHBOT_TOKEN` をそのまま |
| `t` | 13 桁ミリ秒タイムスタンプ |
| `nonce` | UUID v4 |
| `sign` | `base64(HMAC-SHA256(token + t + nonce, secret))` を全大文字化した文字列 |
| `Content-Type` | `application/json` |

SwitchBot v1.1 仕様で base64 結果を **uppercase 化する**点に注意 (標準的 base64 と異なる)。Rust の `String::to_uppercase()` を base64 文字列全体に適用する。base64 は `[A-Za-z0-9+/=]` のみで構成されるため、影響を受けるのは `a-z` のみ (`+` `/` `=` `0-9` は変化なし)。SwitchBot 公式の Python サンプル (`base64.b64encode(...).decode().upper()` 相当) と等価。`signing.rs` のユニットテストで既知ベクトルにより検証する。

### コマンド送信ボディ

```json
{ "command": "...", "parameter": "...", "commandType": "command" }
```

`commandType` は `"command"` 固定。

### Color Bulb のコマンドマッピング

| サブコマンド | command | parameter |
|---|---|---|
| `on` | `turnOn` | `"default"` |
| `off` | `turnOff` | `"default"` |
| `color FEDFE1` | `setColor` | `"254:223:225"` |
| `bright 50` | `setBrightness` | `"50"` |
| `bright max` | `setBrightness` | `"100"` |
| `temp 3000` | `setColorTemperature` | `"3000"` |

### モード判定 (本設計では不要)

GET status で返る Color Bulb の `color` / `colorTemperature` フィールドだけからは「現在どちらのモードがアクティブか」を確実に判別する公式仕様が見つからない。本設計ではローカルの `~/.switchbot/mode` ファイルでモードを追跡するため、status からのモード判定は行わない。

### 成功判定

レスポンス JSON の `statusCode == 100` を成功とみなす。それ以外は `body.message` をエラー詳細として通知 + log に含める。

### レート制限と HTTP エラー

1 token あたり 10,000 calls/day。`bump` 1 回で 2 calls (GET status + POST command) 消費するため、5,000 bump/day までは安全。

レート超過は SwitchBot v1.1 では一般に **HTTP 429** が返る (古い記述で 401 とされている資料もある)。本 CLI では特定の HTTP ステータスに固有の処理を持たず、**HTTP エラー (4xx/5xx) はステータスコードと `body.message` をそのままエラーパスに乗せて通知 + log に出力**する。`statusCode != 100` の正常 JSON レスポンスも同様に `body.message` をエラー扱いする。

### HTTP タイムアウトとリトライ

reqwest クライアントに **5 秒のタイムアウト**を設定する (デフォルトは無限で、ネットワーク断時にハングする)。タイムアウト発生時は通常のエラーパスで通知 + log。

**リトライは行わない**。理由:
- `setColor` `setBrightness` 等は冪等だが、`bump` 系は GET → 計算 → POST のシーケンスでありリトライすると意図しない多重適用が起こりうる
- Stream Deck の指フィードバックを長時間待たせる UX が悪い
- 失敗時はユーザーがもう一度ボタンを押す方が予測可能

## エラーハンドリングとフィードバック

### 失敗パターン × 出力経路

| 失敗の種類 | stderr | 通知 | log |
|---|---|---|---|
| ホームディレクトリ特定不可 (`HOME` 未設定など) | ◯ | × | × |
| `~/.switchbot/.env` 不在 (初回起動) | ◯ | × | × |
| `~/.switchbot/devices` 不在 (初回起動) | ◯ | × | × |
| 1Password 解決失敗 (`op` 未インストール / lock 中 / item 不在) | ◯ | ◯ | ◯ |
| 引数バリデーション (hex 不正、温度範囲外、bump axis 不正) | ◯ | ◯ | ◯ |
| モードファイル未作成での `bump R/G/B/temp±` | ◯ | ◯ | ◯ |
| モード不一致 (`bump R+` in temp mode 等) | ◯ | ◯ | ◯ |
| HTTP タイムアウト (5 秒) | ◯ | ◯ | ◯ |
| API エラー (HTTP 4xx/5xx / `statusCode != 100`) | ◯ | ◯ | ◯ |

設定ファイル不在のときだけ通知を出さない理由: その状況はユーザーが対話的にセットアップしている最中であり、ターミナルで stderr が見えているはずだから。

### ログフォーマット

```
<ISO8601 JST> <LEVEL: INFO|ERROR> <1 行メッセージ>
```

### 通知

`osascript -e 'display notification "<msg>" with title "switchbot"'` を spawn する。osascript 自体の失敗 (まれ) は無視 (失敗の上塗りを避ける)。

## テスト方針

### ユニットテスト (`cargo test`)

| 対象 | 内容 |
|---|---|
| `api::signing` | HMAC-SHA256 + base64 + uppercase の既知ベクトル検証 |
| `commands` の bump 算術 | clamp ロジック (各 axis × 範囲端) |
| `cli` | clap パースの正常系/異常系 (hex 6 桁、温度 2700–6500、bump axis enum) |
| `config` | devices TOML パース、mode TOML の round-trip |

### ユニットテストしないもの

- HTTP 通信 (wiremock 不採用。1 人ツールで維持コストが利得を上回る)
- 1Password 連携 (実環境必須)
- osascript 通知 (macOS GUI 必須)

### 手動 smoke test (README にチェックリスト記載)

実機で以下を順に実行して目視確認:

1. `switchbot list` で `[default]` デバイスが見える
2. `switchbot color FF0000` → 電球が赤
3. `switchbot bump R-` → 赤味がやや下がる
4. `switchbot bright 50` → 暗くなる
5. `switchbot temp 3000` → 温度モード (暖色)
6. `switchbot bump R+` → エラー通知 (モード不一致)
7. `switchbot bump temp+` → 温度が +100K
8. `switchbot bright+` → 明るくなる (温度モードでも有効)
9. `switchbot off` → 消灯
10. `switchbot on` → 点灯

### wiremock を採用しない理由

API 契約変更が起きた場合、手動 smoke test で即発覚する。1 人ツール段階では wiremock の維持コストが得られる早期検出より高い。複数人で触るタイミングで導入する。

## クレート依存

```toml
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
```

`reqwest` は **rustls-tls** + **blocking** で OpenSSL 依存を排除する。非同期ランタイムは不要。

## ビルド・配布

```sh
# リリースビルド
cargo build --release

# ~/.local/bin/switchbot にインストール
cargo install --path . --root ~/.local
```

Stream Deck からの呼び出し側 (`stream-deck-misc` の `.sh`) はフルパス `$HOME/.local/bin/switchbot` を直接書く。Stream Deck 起動時の PATH が貧弱なため。

## 動作環境

- macOS (Apple Silicon / Intel)
- Rust 1.70 以上
- SwitchBot アプリでクラウドサービスを有効化済みの Color Bulb
- 1Password CLI (`op`) v2 以上、PATH 上に配置 — `.env` に `op://` 参照を書く場合のみ必須
- 1Password アプリの biometric unlock を有効化推奨

## 初版に含めない項目

- 複数デバイス対応 (`--device <name>`、`devices` 複数セクション切り替え)
- bump step 幅の設定可能化 (`~/.switchbot/config` 等)
- ログのローテーション・サイズ制限
- インタラクティブな初期セットアップコマンド (`switchbot configure`)
- Color Bulb 以外のデバイスタイプ
- 非同期 / 並列リクエスト
- `op` CLI の代替 (1Password Connect、Service Account 等)

## SPEC.md (v1) からの主な変更点

| 領域 | v1 (SPEC.md) | v2 (本設計) | 理由 |
|---|---|---|---|
| 状態保持 | `~/.switchbot/state` に `r/g/b/brightness` を保持 | 廃止。値は GET status で取得、モードのみ `~/.switchbot/mode` に保持 | ローカル state は外部操作で容易にズレる。GET status を真実の情報源にする |
| `bump` axes | 8 種 (R/G/B/bright × ±) | 10 種 (R/G/B/bright/temp × ±) | 温度モード中の微調整体験のため `temp±` を追加 |
| `bump` のモードチェック | なし | `R/G/B±` は RGB モード時のみ、`temp±` は温度モード時のみ。違反は exit 1 + 通知 | 温度モード中の `bump R+` で古い RGB 値による意図しないモード切替が起きるのを防止 |
| 明るさ step | ±5 | ±10 | Stream Deck で「暗→明」を振る際のテンポを優先 |
| 認証情報の置き場 | `~/.switchbot/secrets` (TOML、平文) | `~/.switchbot/.env` + 1Password 参照 (`op inject` で解決) | 平文保持を避け、1Password 連携を前提とする |
| `list` の出力 | 未定義 (整形のみ) | TOML 形式 (`switchbot list > ~/.switchbot/devices` で完結) | 初回設定の動線を作る |
| モジュール構成 | 8 個並列 (`signing.rs` `notify.rs` `log.rs` 含む) | 7 個 (`signing` を `api/` 配下、`notify`+`log` を `feedback` に統合、`state` 廃止) | 責務の依存関係に沿った構造化 |
| API 呼び出し回数 | コマンド 1 回 | `bump` のみ 2 回 (GET status + POST command) | 真実の情報源を電球側にする代償 |
