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
    #[value(name = "R+")]
    RPlus,
    #[value(name = "R-")]
    RMinus,
    #[value(name = "G+")]
    GPlus,
    #[value(name = "G-")]
    GMinus,
    #[value(name = "B+")]
    BPlus,
    #[value(name = "B-")]
    BMinus,
    #[value(name = "bright+")]
    BrightPlus,
    #[value(name = "bright-")]
    BrightMinus,
    #[value(name = "temp+")]
    TempPlus,
    #[value(name = "temp-")]
    TempMinus,
}

fn parse_hex(s: &str) -> Result<(u8, u8, u8), String> {
    if s.len() != 6 {
        return Err(format!(
            "hex は 6 桁である必要があります (got {} 桁)",
            s.len()
        ));
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
    let n: u32 = s
        .parse()
        .map_err(|_| format!("整数または 'max' を指定してください (got '{}')", s))?;
    if !(1..=100).contains(&n) {
        return Err(format!("明るさは 1-100 の範囲です (got {})", n));
    }
    Ok(n)
}

fn parse_temperature(s: &str) -> Result<u32, String> {
    let n: u32 = s
        .parse()
        .map_err(|_| format!("整数を指定してください (got '{}')", s))?;
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
