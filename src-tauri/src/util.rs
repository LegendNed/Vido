//! Filename templates and path helpers.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Makes a string safe as a single Windows path component. Characters that are
/// invalid in file names *or* awkward in shell commands are replaced with `_`.
pub fn sanitize_component(s: &str, max_chars: usize) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_control()
                || matches!(
                    c,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '%' | '&' | '^' | '!'
                )
            {
                '_'
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    out = out.chars().take(max_chars).collect();
    let trimmed = out.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    let upper = trimmed.to_ascii_uppercase();
    let stem = upper.split('.').next().unwrap_or("");
    if trimmed.is_empty() {
        "clip".into()
    } else if RESERVED.contains(&stem) {
        format!("_{trimmed}")
    } else {
        trimmed
    }
}

/// Renders a validated filename template (without extension).
pub fn render_filename(
    template: &str,
    now: DateTime<Local>,
    source: &str,
    duration_s: u32,
) -> String {
    let name = template
        .replace("{date}", &now.format("%Y-%m-%d").to_string())
        .replace("{time}", &now.format("%H-%M-%S").to_string())
        .replace("{source}", &sanitize_component(source, 40))
        .replace("{duration}", &format!("{duration_s}s"));
    sanitize_component(&name, 150)
}

/// `dir/stem.ext`, or `dir/stem (2).ext`, `(3)`, ... if taken.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    for i in 2..10_000 {
        let p = dir.join(format!("{stem} ({i}).{ext}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-{}.{ext}", uuid::Uuid::new_v4().simple()))
}

/// Moves a file to the Recycle Bin (a recoverable delete).
pub fn move_to_recycle_bin(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        crate::win::recycle_file(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "the Recycle Bin is only available on Windows",
        ))
    }
}

pub const MEDIA_EXTENSIONS: [&str; 4] = ["mp4", "mkv", "webm", "mov"];

pub fn is_media_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| MEDIA_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Seconds → `m:ss` / `h:mm:ss` (for messages).
pub fn fmt_duration(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn renders_default_template() {
        let now = Local.with_ymd_and_hms(2026, 10, 18, 14, 32, 5).unwrap();
        assert_eq!(
            render_filename("Vido_{date}_{time}", now, "x", 30),
            "Vido_2026-10-18_14-32-05"
        );
        assert_eq!(
            render_filename("{source} - {duration}", now, "Game: \"Best\" <Ed.>", 15),
            "Game_ _Best_ _Ed._ - 15s"
        );
    }

    #[test]
    fn sanitizes_reserved_and_empty_names() {
        assert_eq!(sanitize_component("CON", 50), "_CON");
        assert_eq!(sanitize_component("  ...  ", 50), "clip");
        assert_eq!(sanitize_component("a & b", 50), "a _ b");
        assert_eq!(sanitize_component(&"x".repeat(100), 10).len(), 10);
    }

    #[test]
    fn unique_paths_do_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let a = unique_path(dir.path(), "clip", "mp4");
        std::fs::write(&a, b"x").unwrap();
        let b = unique_path(dir.path(), "clip", "mp4");
        assert_eq!(b.file_name().unwrap(), "clip (2).mp4");
    }

    #[test]
    fn durations() {
        assert_eq!(fmt_duration(65.4), "1:05");
        assert_eq!(fmt_duration(3725.0), "1:02:05");
    }
}
