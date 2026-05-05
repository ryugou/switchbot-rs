use anyhow::{anyhow, Result};

use crate::api::{self, parse_color_str, Client};
use crate::cli::{BumpAxis, Command};
use crate::config::{self, Context, Mode};

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

pub fn handle(command: &Command, ctx: &Context) -> Result<String> {
    let client = Client::new(
        ctx.credentials.token.clone(),
        ctx.credentials.secret.clone(),
    )?;
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
                AxisDelta::Red(_) => (bump_rgb_channel(r0, d), g0, b0),
                AxisDelta::Green(_) => (r0, bump_rgb_channel(g0, d), b0),
                AxisDelta::Blue(_) => (r0, g0, bump_rgb_channel(b0, d)),
                _ => unreachable!("outer match arm guarantees Red|Green|Blue"),
            };
            client.set_color(&ctx.device.id, r, g, b)?;
            Ok(format!("bump {} ok ({}:{}:{})", axis_label(axis), r, g, b))
        }
        AxisDelta::Brightness(d) => {
            let status = client.get_status(&ctx.device.id)?;
            let new_value = bump_brightness(status.brightness, d);
            client.set_brightness(&ctx.device.id, new_value)?;
            Ok(format!("bump {} ok ({})", axis_label(axis), new_value))
        }
        AxisDelta::Temperature(d) => {
            require_mode(mode, Mode::Temp)?;
            let status = client.get_status(&ctx.device.id)?;
            let new_k = bump_temperature(status.color_temperature, d);
            client.set_color_temperature(&ctx.device.id, new_k)?;
            Ok(format!("bump {} ok ({}K)", axis_label(axis), new_k))
        }
    }
}

/// list 出力用に Device 配列を TOML 形式に整形する。
/// 1 台なら [default]、複数なら deviceName を kebab-case 化したセクション名にする。
pub fn format_devices_toml(devices: &[api::Device]) -> String {
    let single = devices.len() == 1;
    let mut out = String::new();
    for (i, d) in devices.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let section = if single {
            "default".to_string()
        } else {
            sanitize_section_key(&d.name)
        };
        out.push_str(&format!("[{}]\n", section));
        out.push_str(&format!("id = \"{}\"\n", d.id));
        out.push_str(&format!("type = \"{}\"\n", d.kind));
        out.push_str(&format!("name = \"{}\"\n", d.name));
    }
    out
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
}
