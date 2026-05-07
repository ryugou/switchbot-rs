use std::collections::HashSet;

use anyhow::{anyhow, Result};
use toml::value::{Table, Value};

use crate::api::{self, parse_color_str, Client};
use crate::cli::{BumpAxis, Command};
use crate::config::{self, Context, DefaultDevice, DeviceState, Mode};

pub const RGB_STEP: i32 = 16;
pub const BRIGHT_STEP: i32 = 10;
pub const TEMP_STEP: i32 = 100;

pub fn bump_rgb_channel(current: u8, delta: i32) -> u8 {
    (current as i32 + delta).clamp(0, 255) as u8
}

pub fn bump_brightness(current: u32, delta: i32) -> u32 {
    (current as i32 + delta).clamp(1, 100) as u32
}

pub fn bump_temperature(current: u32, delta: i32) -> u32 {
    (current as i32 + delta).clamp(2700, 6500) as u32
}

fn axis_label(axis: BumpAxis) -> &'static str {
    use BumpAxis::*;
    match axis {
        RPlus => "R+",
        RMinus => "R-",
        GPlus => "G+",
        GMinus => "G-",
        BPlus => "B+",
        BMinus => "B-",
        BrightPlus => "bright+",
        BrightMinus => "bright-",
        TempPlus => "temp+",
        TempMinus => "temp-",
    }
}

pub fn axis_delta(axis: BumpAxis) -> AxisDelta {
    use BumpAxis::*;
    match axis {
        RPlus => AxisDelta::Red(RGB_STEP),
        RMinus => AxisDelta::Red(-RGB_STEP),
        GPlus => AxisDelta::Green(RGB_STEP),
        GMinus => AxisDelta::Green(-RGB_STEP),
        BPlus => AxisDelta::Blue(RGB_STEP),
        BMinus => AxisDelta::Blue(-RGB_STEP),
        BrightPlus => AxisDelta::Brightness(BRIGHT_STEP),
        BrightMinus => AxisDelta::Brightness(-BRIGHT_STEP),
        TempPlus => AxisDelta::Temperature(TEMP_STEP),
        TempMinus => AxisDelta::Temperature(-TEMP_STEP),
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

/// `Mode` を公開ラベル ("rgb" / "temp") に変換する。CLI 出力と JSON の両方で使う。
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

/// 期待モードと実際モードを照合し、ズレていればエラーを返す。
pub fn require_mode(actual: Option<Mode>, expected: Mode) -> Result<()> {
    match actual {
        None => Err(anyhow!(
            "モードが未設定です。先に switchbot color <hex> または switchbot temp <K> を実行してください。"
        )),
        Some(m) if m == expected => Ok(()),
        Some(_) => {
            let bin = env!("CARGO_PKG_NAME");
            let (current_label, switch_cmd) = match expected {
                Mode::Rgb => ("温度モード", format!("{bin} color <hex>")),
                Mode::Temp => ("RGB モード", format!("{bin} temp <K>")),
            };
            Err(anyhow!(
                "現在 {}です。{} を先に実行してください。",
                current_label,
                switch_cmd
            ))
        }
    }
}

/// device が必要なコマンドで ctx.device が Configured でない場合にエラーを返す。
fn require_device(ctx: &Context) -> Result<&DefaultDevice> {
    match &ctx.device {
        DeviceState::Configured(d) => Ok(d),
        DeviceState::Unconfigured => Err(anyhow!(
            "デバイスが未設定です。`switchbot list > ~/.switchbot/devices` を実行し、\
             使うデバイスのセクション名を [default] にしてください \
             (デバイスが 1 台のみなら自動で [default] になります)。"
        )),
        DeviceState::Malformed(msg) => Err(anyhow!(
            "~/.switchbot/devices の解析に失敗しました: {}\n\
             `switchbot list > ~/.switchbot/devices` で再生成できます。",
            msg
        )),
    }
}

pub fn handle(command: &Command, ctx: &Context) -> Result<String> {
    let client = Client::new(
        ctx.credentials.token.clone(),
        ctx.credentials.secret.clone(),
    )?;
    match command {
        Command::List => cmd_list(&client),
        Command::Mode => {
            let device = require_device(ctx)?;
            cmd_mode(&client, ctx, device)
        }
        Command::Status => {
            let device = require_device(ctx)?;
            cmd_status(&client, device)
        }
        Command::Sync => Ok(String::new()), // Task 5 で実装
        Command::Color { rgb: (r, g, b) } => {
            let device = require_device(ctx)?;
            cmd_color(&client, ctx, device, *r, *g, *b)
        }
        Command::Bright { value } => {
            let device = require_device(ctx)?;
            cmd_bright(&client, device, *value)
        }
        Command::Temp { kelvin } => {
            let device = require_device(ctx)?;
            cmd_temp(&client, ctx, device, *kelvin)
        }
        Command::Bump { axis } => {
            let device = require_device(ctx)?;
            cmd_bump(&client, ctx, device, *axis)
        }
        Command::On => {
            let device = require_device(ctx)?;
            cmd_on(&client, device)
        }
        Command::Off => {
            let device = require_device(ctx)?;
            cmd_off(&client, device)
        }
    }
}

fn cmd_color(
    client: &Client,
    ctx: &Context,
    device: &DefaultDevice,
    r: u8,
    g: u8,
    b: u8,
) -> Result<String> {
    client.set_color(&device.id, r, g, b)?;
    config::write_mode(&ctx.mode_path, Mode::Rgb)?;
    Ok(format!("color {:02X}{:02X}{:02X} ok", r, g, b))
}

fn cmd_bright(client: &Client, device: &DefaultDevice, value: u32) -> Result<String> {
    client.set_brightness(&device.id, value)?;
    Ok(format!("bright {} ok", value))
}

fn cmd_temp(client: &Client, ctx: &Context, device: &DefaultDevice, kelvin: u32) -> Result<String> {
    client.set_color_temperature(&device.id, kelvin)?;
    config::write_mode(&ctx.mode_path, Mode::Temp)?;
    Ok(format!("temp {} ok", kelvin))
}

fn cmd_on(client: &Client, device: &DefaultDevice) -> Result<String> {
    client.turn_on(&device.id)?;
    Ok("on ok".to_string())
}

fn cmd_off(client: &Client, device: &DefaultDevice) -> Result<String> {
    client.turn_off(&device.id)?;
    Ok("off ok".to_string())
}

fn cmd_list(client: &Client) -> Result<String> {
    let devices = client.list_devices()?;
    Ok(format_devices_toml(&devices))
}

fn cmd_status(client: &Client, device: &DefaultDevice) -> Result<String> {
    let status = client.get_status(&device.id)?;
    format_status_json(&status)
}

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

/// list 出力用に Device 配列を TOML 形式に整形する。
/// toml クレート経由で生成するため、特殊文字が正しくエスケープされる。
/// 1 台なら [default]、複数なら deviceName を kebab-case 化したセクション名にする。
/// セクション名が衝突する場合は "-2", "-3" ... を付与して一意にする。
pub fn format_devices_toml(devices: &[api::Device]) -> String {
    let single = devices.len() == 1;
    let mut sections: Vec<(String, Table)> = Vec::with_capacity(devices.len());
    let mut seen: HashSet<String> = HashSet::new();

    for d in devices {
        let base_key = if single {
            "default".to_string()
        } else {
            sanitize_section_key(&d.name)
        };
        let key = unique_key(&base_key, &mut seen);

        let mut table = Table::new();
        table.insert("id".to_string(), Value::String(d.id.clone()));
        table.insert("type".to_string(), Value::String(d.kind.clone()));
        table.insert("name".to_string(), Value::String(d.name.clone()));
        sections.push((key, table));
    }

    let mut out = String::new();
    for (i, (key, table)) in sections.into_iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut wrapper = Table::new();
        wrapper.insert(key, Value::Table(table));
        out.push_str(
            &toml::to_string(&Value::Table(wrapper))
                .expect("toml::to_string of String/Table never fails"),
        );
    }
    out
}

fn unique_key(base: &str, seen: &mut HashSet<String>) -> String {
    if seen.insert(base.to_string()) {
        return base.to_string();
    }
    let mut counter: u32 = 2;
    loop {
        let candidate = format!("{}-{}", base, counter);
        if seen.insert(candidate.clone()) {
            return candidate;
        }
        counter = counter.saturating_add(1);
        if counter == u32::MAX {
            // 実用上ありえないが、無限ループ回避のため最後の手段
            return format!("{}-{}", base, uuid::Uuid::new_v4());
        }
    }
}

fn sanitize_section_key(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = true; // 先頭のハイフンを除去するため true で始める
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_end_matches('-');
    if trimmed.is_empty() {
        "device".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            axis_delta(BumpAxis::TempMinus),
            AxisDelta::Temperature(-100)
        );
    }

    #[test]
    fn axis_label_uses_user_facing_labels() {
        assert_eq!(axis_label(BumpAxis::RPlus), "R+");
        assert_eq!(axis_label(BumpAxis::RMinus), "R-");
        assert_eq!(axis_label(BumpAxis::BrightPlus), "bright+");
        assert_eq!(axis_label(BumpAxis::TempMinus), "temp-");
    }

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
        assert_eq!(
            sanitize_section_key("Bedroom Plug Mini"),
            "bedroom-plug-mini"
        );
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

    // --- 修正 3 テスト ---

    #[test]
    fn format_devices_escapes_special_characters() {
        let devices = vec![api::Device {
            id: "01-x".to_string(),
            name: "Living \"Smart\" Bulb\nNew Line".to_string(),
            kind: "Color Bulb".to_string(),
        }];
        let out = format_devices_toml(&devices);
        // toml::to_string は " と \n を含む文字列を安全にエンコードする。
        // 具体的には multiline 基本文字列 ("""...""") またはエスケープシーケンス形式で出力される。
        // いずれにせよ元の文字列を TOML デシリアライズで復元できることを確認する。
        let parsed: toml::Value =
            toml::from_str(&out).expect("format_devices_toml must produce valid TOML");
        let name = parsed["default"]["name"]
            .as_str()
            .expect("name must be a string");
        assert_eq!(name, "Living \"Smart\" Bulb\nNew Line");
    }

    #[test]
    fn format_devices_unique_keys_on_collision() {
        let devices = vec![
            api::Device {
                id: "01".to_string(),
                name: "Bulb".to_string(),
                kind: "Color Bulb".to_string(),
            },
            api::Device {
                id: "02".to_string(),
                name: "Bulb".to_string(), // 同名
                kind: "Color Bulb".to_string(),
            },
        ];
        let out = format_devices_toml(&devices);
        assert!(out.contains("[bulb]"), "first should be [bulb]: {}", out);
        assert!(
            out.contains("[bulb-2]"),
            "second should be [bulb-2]: {}",
            out
        );
    }

    #[test]
    fn unique_key_increments_indefinitely() {
        let mut seen = std::collections::HashSet::new();
        seen.insert("base".to_string());
        seen.insert("base-2".to_string());
        seen.insert("base-3".to_string());
        let key = unique_key("base", &mut seen);
        assert_eq!(key, "base-4");
    }

    #[test]
    fn unique_key_handles_collision_chain() {
        // base, base-2, base-3, ... base-1000 全て埋まっている状況
        let mut seen = std::collections::HashSet::new();
        seen.insert("base".to_string());
        for n in 2..=1000 {
            seen.insert(format!("base-{}", n));
        }
        let key = unique_key("base", &mut seen);
        assert_eq!(key, "base-1001");
    }

    // --- infer_mode_from_status テスト ---

    use crate::api::BulbStatus;

    fn make_status(color: &str, color_temperature: u32) -> BulbStatus {
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
        let s = make_status("0:0:0", 4500);
        assert_eq!(infer_mode_from_status(&s), Mode::Temp);
    }

    #[test]
    fn format_status_json_rgb_mode() {
        let s = make_status("255:128:0", 0);
        let out = format_status_json(&s).unwrap();
        // フィールド順序は実装に依存しないので含有チェックのみ
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
        assert!(
            !out.contains('\n'),
            "JSON should be single line, got: {}",
            out
        );
    }
}
