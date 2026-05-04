mod signing;

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::Deserialize;
use std::time::Duration;

const BASE_URL: &str = "https://api.switch-bot.com";

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

pub struct Client {
    pub token: String,
    pub secret: String,
    http: reqwest::blocking::Client,
}

impl Client {
    pub fn new(token: String, secret: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            token,
            secret,
            http,
        })
    }

    fn auth_headers(&self) -> Result<HeaderMap> {
        let t = chrono::Utc::now().timestamp_millis();
        let nonce = uuid::Uuid::new_v4().to_string();
        let sign = signing::compute_sign(&self.token, &self.secret, t, &nonce);
        Self::build_headers(&self.token, t, &nonce, &sign)
    }

    fn build_headers(token: &str, t: i64, nonce: &str, sign: &str) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", HeaderValue::from_str(token)?);
        headers.insert("sign", HeaderValue::from_str(sign)?);
        headers.insert("t", HeaderValue::from_str(&t.to_string())?);
        headers.insert("nonce", HeaderValue::from_str(nonce)?);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }
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

    #[test]
    fn build_headers_contains_all_required() {
        let headers = Client::build_headers("tok", 1234567890, "noncex", "ABCD1234").unwrap();
        assert_eq!(headers.get("Authorization").unwrap(), "tok");
        assert_eq!(headers.get("sign").unwrap(), "ABCD1234");
        assert_eq!(headers.get("t").unwrap(), "1234567890");
        assert_eq!(headers.get("nonce").unwrap(), "noncex");
        assert_eq!(headers.get("content-type").unwrap(), "application/json");
    }

    #[test]
    fn client_new_with_5s_timeout_builds() {
        let client = Client::new("tok".to_string(), "sec".to_string());
        assert!(client.is_ok());
    }
}
