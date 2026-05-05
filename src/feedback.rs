use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::Command;

pub fn log_info(log_path: &Path, msg: &str) {
    write_log(log_path, "INFO", msg);
}

pub fn log_error(log_path: &Path, msg: &str) {
    write_log(log_path, "ERROR", msg);
}

fn write_log(log_path: &Path, level: &str, msg: &str) {
    let line = format_log_line(chrono::Local::now(), level, msg);
    let _ = OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

fn format_log_line(now: chrono::DateTime<chrono::Local>, level: &str, msg: &str) -> String {
    format!(
        "{} {:5} {}\n",
        now.format("%Y-%m-%dT%H:%M:%S%:z"),
        level,
        msg.lines().next().unwrap_or(""),
    )
}

pub fn notify(msg: &str) {
    let escaped = escape_for_applescript(msg);
    let _ = Command::new("osascript")
        .arg("-e")
        .arg(format!(
            "display notification \"{}\" with title \"switchbot\"",
            escaped
        ))
        .status();
}

fn escape_for_applescript(s: &str) -> String {
    s.replace('\\', "\\\\") // バックスラッシュを最初に
        .replace('"', "\\\"") // 次に引用符
        .replace('\r', "") // CR は除去
        .replace('\n', " ") // LF は空白に
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::tempdir;

    #[test]
    fn log_appends_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("log");
        log_info(&path, "first message");
        log_info(&path, "second message");
        log_error(&path, "third message");
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("INFO"));
        assert!(lines[0].contains("first message"));
        assert!(lines[2].contains("ERROR"));
        assert!(lines[2].contains("third message"));
    }

    #[test]
    fn log_format_has_iso8601_timestamp() {
        let now = chrono::Local
            .with_ymd_and_hms(2026, 5, 4, 12, 34, 56)
            .unwrap();
        let line = format_log_line(now, "INFO", "hello");
        assert!(line.starts_with("2026-05-04T12:34:56"));
        assert!(line.contains(" INFO  hello"));
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn log_strips_extra_lines_in_message() {
        let now = chrono::Local
            .with_ymd_and_hms(2026, 5, 4, 12, 34, 56)
            .unwrap();
        let line = format_log_line(now, "ERROR", "first line\nsecond line");
        assert!(line.contains("first line"));
        assert!(!line.contains("second line"));
    }

    #[test]
    fn applescript_escape_doubles_backslash_and_quote() {
        assert_eq!(escape_for_applescript(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(escape_for_applescript("plain"), "plain");
    }

    #[test]
    fn applescript_escape_replaces_newlines_with_space() {
        assert_eq!(escape_for_applescript("line1\nline2"), "line1 line2");
        assert_eq!(escape_for_applescript("a\nb\nc"), "a b c");
    }

    #[test]
    fn applescript_escape_strips_carriage_returns() {
        assert_eq!(escape_for_applescript("line1\r\nline2"), "line1 line2");
        assert_eq!(escape_for_applescript("a\rb"), "ab");
    }

    #[test]
    fn applescript_escape_handles_combined() {
        // バックスラッシュ・引用符・改行が混在
        assert_eq!(
            escape_for_applescript("error: \"bad\" input\nusage: ..."),
            r#"error: \"bad\" input usage: ..."#
        );
    }
}
