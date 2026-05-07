# switchbot-rs

SwitchBot Color Bulb (W1401400) を Stream Deck から操作するための Rust 製シングルバイナリ CLI。

## 特徴

- 単一バイナリ (~4-5 MB)。`~/.local/bin/switchbot` に置くだけ
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
switchbot mode                   # 現在のモードを 1 行で出力 (rgb / temp)
switchbot status                 # 電球の現状を JSON で出力
switchbot sync                   # API 状態をローカル mode に反映
```

`bump` の axes:
- RGB: `R+`, `R-`, `G+`, `G-`, `B+`, `B-` (RGB モード時のみ。±16)
- 明るさ: `bright+`, `bright-` (両モード可。±10)
- 色温度: `temp+`, `temp-` (温度モード時のみ。±100K)

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
- [ ] `switchbot mode` で `rgb` または `temp` が出力される (ローカル mode 有り)
- [ ] `~/.switchbot/mode` を削除 → `switchbot mode` で API ヒューリスティクスから推測される値が出力される
- [ ] `switchbot status` で JSON が出力される (`jq` でパース可能)
- [ ] 公式アプリで電球のモードを変える → `switchbot sync` → `cat ~/.switchbot/mode` で値が更新されている
- [ ] `~/.switchbot/mode` を削除 → `switchbot bump R+` (RGB 状態時) が「モード未設定」エラーで弾かれず動作する

## 仕様書

- 設計 (v2): `docs/superpowers/specs/2026-05-04-switchbot-rs-design.md`
- 実装計画: `docs/superpowers/plans/2026-05-05-switchbot-rs-implementation.md`
- 旧仕様 (v1, 参考): `docs/SPEC.md`
