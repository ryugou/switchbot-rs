use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// SwitchBot v1.1 仕様の sign ヘッダ値を計算する。
///
/// `base64(HMAC-SHA256(token + t + nonce, secret))` を全大文字化して返す。
pub fn compute_sign(token: &str, secret: &str, t: i64, nonce: &str) -> String {
    let data = format!("{}{}{}", token, t, nonce);
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(data.as_bytes());
    let result = mac.finalize().into_bytes();
    base64::engine::general_purpose::STANDARD
        .encode(result)
        .to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let a = compute_sign("token", "secret", 1234567890, "nonce");
        let b = compute_sign("token", "secret", 1234567890, "nonce");
        assert_eq!(a, b);
    }

    #[test]
    fn different_input_different_output() {
        let a = compute_sign("token1", "secret", 1234567890, "nonce");
        let b = compute_sign("token2", "secret", 1234567890, "nonce");
        assert_ne!(a, b);
    }

    #[test]
    fn output_has_no_lowercase() {
        let s = compute_sign("token", "secret", 1234567890, "nonce");
        assert!(
            s.chars().all(|c| !c.is_ascii_lowercase()),
            "expected no lowercase letters, got: {}",
            s
        );
    }

    #[test]
    fn output_length_44() {
        // base64 of 32 bytes (HMAC-SHA256 output) = 44 chars including padding
        let s = compute_sign("token", "secret", 1234567890, "nonce");
        assert_eq!(s.len(), 44);
    }

    #[test]
    fn matches_reference_python_vector() {
        // Reference value computed via:
        //   python3 -c "import hmac, hashlib, base64; \
        //     data=b'test_token1635146797241test-nonce'; \
        //     k=b'test_secret'; \
        //     print(base64.b64encode(hmac.new(k, data, hashlib.sha256).digest()).decode().upper())"
        let expected = "T0/YPYSTPMPKAS1VEE+VCYNNOSK+2V2ECQX6OTHADPU=";
        let actual = compute_sign("test_token", "test_secret", 1635146797241, "test-nonce");
        assert_eq!(actual, expected);
    }
}
