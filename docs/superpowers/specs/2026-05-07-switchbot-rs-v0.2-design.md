# switchbot-rs v0.2 設計書: mode / status / sync

> 本ドキュメントは v0.2 で追加する 3 つのコマンドと、`bump` のモード判定ロジック更新を定義する。v0.1 の設計書 (`2026-05-04-switchbot-rs-design.md`) を補足する形で読む。

## 背景

v0.1 では `bump R/G/B/temp±` のモード判定にローカル `~/.switchbot/mode` ファイルだけを使っていた。これによる課題:

1. 初回起動 (mode ファイル無し) では `bump R/G/B/temp±` が「モード未設定」エラーで弾かれる
2. 別端末・公式アプリでモードを変えるとローカル mode とデバイスがズレる
3. 「現在のモード」「電球の現状」を CLI で確認する手段が無い

実機検証 (2026-05-07) で SwitchBot Color Bulb の API は **アクティブでないモードの値をゼロクリアする** ことが判明:

| 状態 | `color` | `colorTemperature` |
|---|---|---|
| 温度モード (例: 3100K) | `"0:0:0"` | `3100` |
| RGB モード (例: 赤) | `"255:0:0"` | `0` |

これにより `colorTemperature == 0` で RGB / temp を判定できる。**公式仕様ではないが観測ベースで実機 (Color Bulb V2.0-2.0) で安定**。

## 追加する CLI コマンド

### `switchbot mode`

現在のモードを 1 行プレーンテキストで返す。

**判定ロジック:**

1. ローカル `~/.switchbot/mode` が存在し有効値 (`rgb` / `temp`) なら、その値を返す
2. 無ければ API `GET /v1.1/devices/{id}/status` を呼び、`colorTemperature` で判定:
   - `colorTemperature == 0` → `rgb`
   - `colorTemperature > 0` → `temp`

ローカル mode ファイルが parse error の場合は通常エラーパスへ (mode コマンドの結果としては失敗)。

**出力:**
- stdout: `rgb\n` または `temp\n`
- stderr: なし
- exit: 0

**エラーパス:** API 失敗、ローカル mode parse error 等は通常エラー (stderr + log + notify + exit 1)。

### `switchbot status`

API で実機のフル情報を取得して JSON で出力。

**JSON フィールド:**

| フィールド | 型 | 値の出所 |
|---|---|---|
| `power` | string | API `power` (`"on"` / `"off"`) |
| `brightness` | number | API `brightness` (1–100) |
| `color` | string | API `color` (例: `"255:128:0"`) |
| `color_temperature` | number | API `colorTemperature` (Kelvin、温度モードでなければ 0) |
| `mode` | string | API ヒューリスティクスで導出 (`"rgb"` / `"temp"`) |

**出力例:**

```json
{"power":"on","brightness":50,"color":"255:0:0","color_temperature":0,"mode":"rgb"}
```

末尾に改行 1 つ。1 行 (人間向け整形なし、`jq` でパース可能)。

**エラーパス:** API 失敗は通常エラー (stderr + log + notify + exit 1)。

### `switchbot sync`

API status を取得し、ローカル `~/.switchbot/mode` を実機状態に合わせて上書きする。

**用途:** 別端末・公式アプリでモードが変えられた後に、ローカル mode を再同期する。

**ロジック:**
1. API `GET /v1.1/devices/{id}/status`
2. `colorTemperature == 0` なら `Mode::Rgb`、それ以外なら `Mode::Temp`
3. ローカル mode ファイルを atomic に書き込み (既存の `write_mode` を再利用)

**出力:**
- stdout: なし
- stderr: なし (成功時は無音)
- log: `INFO sync ok (rgb)` のような短文
- exit: 0 (成功) / 1 (API 失敗)

**エラーパス:** API 失敗は通常エラー。

## `bump` のモード判定ロジック更新

### 変更前 (v0.1)

`cmd_bump` は最初に `read_mode()` を呼び、`Option<Mode>` を取得。`None` の場合は「モード未設定」エラーで `require_mode` を失敗させていた。

### 変更後 (v0.2)

`cmd_bump` は既に全 axes で `client.get_status()` を呼んでいる (現在値の取得のため)。**そのレスポンスから `colorTemperature` を見て mode を infer** すれば、ローカル無しでも判定可能。**追加の API コールはゼロ**。

**フロー:**

```
cmd_bump(client, ctx, axis):
  1. ローカル mode を読む (Option<Mode>)
  2. status = client.get_status(...)
  3. mode = local_mode.unwrap_or_else(|| infer_mode_from_status(&status))
  4. require_mode(Some(mode), expected_for_axis)?
  5. status から現在値 (R/G/B / brightness / colorTemperature) を取得
  6. clamp + set_color / set_brightness / set_color_temperature
```

`infer_mode_from_status(status: &BulbStatus) -> Mode`:

```rust
if status.color_temperature == 0 {
    Mode::Rgb
} else {
    Mode::Temp
}
```

**ローカル mode が存在する場合の挙動は変わらない**。「ローカル mode 優先 → 無ければ API infer」のフォールバック動作なので、本 CLI で `color`/`temp` した直後は引き続きローカルが正。

### 「モード未設定」エラーは消える

ローカル無し + API status 取得成功なら、必ず `Mode::Rgb` か `Mode::Temp` が判定される。よって `bump R/G/B/temp±` の「モード未設定」エラーは v0.2 で消える。

ただし `require_mode` のエラー (例: 温度モード中の `bump R+`) は引き続き発生する。

## モジュール構成への影響

| ファイル | 変更内容 |
|---|---|
| `src/cli.rs` | `Command::Mode`, `Command::Status`, `Command::Sync` を追加 |
| `src/commands.rs` | `cmd_mode`, `cmd_status`, `cmd_sync` を追加。`cmd_bump` を新ロジックに更新。`infer_mode_from_status` ヘルパー追加 |
| `src/api/mod.rs` | 変更なし (`BulbStatus` は既存) |
| `src/config.rs` | 変更なし (`read_mode`, `write_mode`, `Mode` 既存) |
| `src/main.rs` | 変更なし (新コマンドは `commands::handle` 経由) |
| `src/feedback.rs` | 変更なし |

## API 呼び出し回数への影響

| コマンド | v0.1 | v0.2 |
|---|---|---|
| `mode` | (新規) | ローカル mode 有り → 0 / 無し → 1 (GET status) |
| `status` | (新規) | 1 (GET status) |
| `sync` | (新規) | 1 (GET status) |
| `bump R/G/B/bright/temp±` | 1 (GET status) + 1 (set コマンド) = 2 | 同じ (status を mode 判定にも流用) |

`bump` の API コール数は変化なし。新コマンド `mode` (ローカル無しケース) `status` `sync` で各 1 コール追加されるが、Stream Deck の通常使用 (1 ボタン押下 = 1 コマンド実行) では問題にならない。

## エラーパスのまとめ

| 失敗パターン | stderr | 通知 | log |
|---|---|---|---|
| `mode` コマンドで API 失敗 (ローカル無しケース) | ◯ | ◯ | ◯ |
| `mode` コマンドでローカル parse error | ◯ | ◯ | ◯ |
| `status` で API 失敗 | ◯ | ◯ | ◯ |
| `sync` で API 失敗 | ◯ | ◯ | ◯ |
| `sync` でローカル mode 書き込み失敗 | ◯ | ◯ | ◯ |
| `bump` の API 失敗・モード不一致 | ◯ | ◯ | ◯ |

通常 v0.1 と同じパターン。

## API 伝播ラグについて (注意点)

実機検証で確認した API のキャッシュ/伝播ラグは **約 5 秒**。

**影響:**
- 別端末で `temp` → 直後に本 CLI で `bump R+` の場合、API status が古い (まだ RGB モード) かもしれず、誤って `bump R+` が許可される可能性がある (~5 秒以内)
- 本 CLI で `temp` 成功 → ローカル mode が `temp` に更新 → 直後の `bump R+` はローカル mode (新値) で判定するため問題なし

**結論:** 別端末との並行運用時にのみラグ問題が顕在化する。Stream Deck 中心の運用では実害なし。`sync` コマンドで明示的に再同期できるので、必要時にユーザーが手動で sync すれば良い。

## テスト方針

### ユニットテスト

| 対象 | テスト内容 |
|---|---|
| `infer_mode_from_status` | `colorTemperature == 0` → Rgb、`>0` → Temp、境界値 |
| `cli` パース | `mode`, `status`, `sync` サブコマンドの認識 |
| `cmd_bump` のモード判定 | ローカル mode 有り (使う) / 無し (API infer 使う) のフォールバック |

### 手動 smoke test (README 追記)

- `switchbot mode` (ローカル mode 有り) → ローカル値を返す
- `~/.switchbot/mode` を削除 → `switchbot mode` → API ヒューリスティクスで返す
- `switchbot status` → JSON 出力
- 公式アプリでモード変更 → `switchbot sync` → ローカル mode が更新される
- `~/.switchbot/mode` を削除 → `switchbot bump R+` → API infer で動く (v0.1 ではエラーだった)

## 互換性

- v0.1 の既存コマンド (`color` / `bright` / `temp` / `bump` / `on` / `off` / `list`) の挙動は変わらない
- ローカル mode ファイルのフォーマットは変更なし (`mode = "rgb" | "temp"`)
- 設定ファイル (`~/.switchbot/.env`, `devices`, `log`) のフォーマットは変更なし
- v0.1 のユーザーは `cargo install --path . --root ~/.local --force` で v0.2 にアップグレードできる (差分なくシームレス)

## ヒューリスティクスの公式仕様性に関する注記

`colorTemperature == 0` での mode 判定は SwitchBot 公式 API ドキュメントには明記されていない。実機 (Color Bulb V2.0-2.0、deviceId `84F703AE7C06`) で観測された挙動である。SwitchBot 側のファームウェア/API 変更でこの挙動が変わる可能性は否定できない。

将来この挙動が変わった場合のフォールバック:
- `bump` のモード判定はローカル mode 優先なので、本 CLI で `color`/`temp` を実行する運用なら影響なし
- `mode` コマンドのローカル無しケース、`sync` コマンドはヒューリスティクスに依存する
- 影響範囲は限定的。仕様変更時は API レスポンスの新フィールド (もしあれば) を見るように更新する

## 将来拡張 (v0.3 以降の候補)

- `status --watch`: 一定間隔で status をポーリングして表示
- `mode --infer-only`: ローカルを無視して常に API ヒューリスティクスで判定
- 複数デバイス対応 (`--device <name>`)
- 他デバイスタイプの追加 (Plug Mini 等) — 各デバイスごとに `status` の JSON 形式が変わるので enum 化が必要

これらは v0.2 のスコープに含めない。
