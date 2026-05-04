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
    let r: u8 = parts[0]
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid R in '{}'", s))?;
    let g: u8 = parts[1]
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid G in '{}'", s))?;
    let b: u8 = parts[2]
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid B in '{}'", s))?;
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
