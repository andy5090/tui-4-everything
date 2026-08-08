use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn run_search(encoded_query: &str) -> Result<()> {
    let query = decode_search_query(encoded_query)?;
    eprintln!("[T4E] Searching YouTube for {query:?}...");
    let output = Command::new(yt_dlp_program())
        .args([
            "--flat-playlist",
            "--playlist-end",
            "1",
            "--no-warnings",
            "--print",
            "webpage_url",
        ])
        .arg(format!("ytsearch1:{query}"))
        .output()
        .context("could not start yt-dlp for the bounded tplay search")?;
    if !output.status.success() {
        bail!(
            "yt-dlp search failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let url = first_youtube_url(&output.stdout)?;
    eprintln!("[T4E] Opening {url} in tplay...");
    let status = Command::new(tplay_program())
        .arg(url)
        .status()
        .context("could not start tplay")?;
    if !status.success() {
        bail!("tplay exited with {status}");
    }
    Ok(())
}

fn decode_search_query(encoded: &str) -> Result<String> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some(high) = bytes.get(index + 1).and_then(|byte| hex_value(*byte)) else {
                bail!("invalid percent-encoded tplay search query");
            };
            let Some(low) = bytes.get(index + 2).and_then(|byte| hex_value(*byte)) else {
                bail!("invalid percent-encoded tplay search query");
            };
            decoded.push((high << 4) | low);
            index += 3;
        } else if bytes[index].is_ascii_alphanumeric()
            || matches!(bytes[index], b'-' | b'_' | b'.' | b'~')
        {
            decoded.push(bytes[index]);
            index += 1;
        } else {
            bail!("unsafe character in encoded tplay search query");
        }
    }
    let query = String::from_utf8(decoded).context("tplay search query is not valid UTF-8")?;
    let query = query.split_whitespace().collect::<Vec<_>>().join(" ");
    if query.is_empty() || query.chars().count() > 160 {
        bail!("tplay search query must contain 1 to 160 characters");
    }
    Ok(query)
}

fn first_youtube_url(stdout: &[u8]) -> Result<String> {
    let stdout = String::from_utf8_lossy(stdout);
    let url = stdout.lines().map(str::trim).find(|line| !line.is_empty());
    let Some(url) = url else {
        bail!("yt-dlp search returned no videos");
    };
    let youtube_url = url.starts_with("https://www.youtube.com/")
        || url.starts_with("https://youtube.com/")
        || url.starts_with("https://youtu.be/");
    if !youtube_url || url.chars().any(char::is_whitespace) {
        bail!("yt-dlp search returned an unsupported URL");
    }
    Ok(url.to_string())
}

fn yt_dlp_program() -> PathBuf {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    if let Some(data_home) = data_home {
        let managed = data_home.join("t4e/tplay/yt-dlp/bin/yt-dlp");
        if managed.is_file() {
            return managed;
        }
    }
    PathBuf::from("yt-dlp")
}

fn tplay_program() -> &'static str {
    if cfg!(target_os = "linux") {
        "t4e-tplay"
    } else {
        "tplay"
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_search_query, first_youtube_url};

    #[test]
    fn decodes_a_bounded_unicode_search_query() {
        assert_eq!(
            decode_search_query("%EC%98%81%EC%83%81%EB%AF%B8%20cinematic%204K")
                .expect("valid query"),
            "영상미 cinematic 4K"
        );
        assert!(decode_search_query("bad;query").is_err());
        assert!(decode_search_query("%GG").is_err());
    }

    #[test]
    fn accepts_only_one_resolved_youtube_url() {
        assert_eq!(
            first_youtube_url(b"https://www.youtube.com/watch?v=abc123\n").expect("YouTube URL"),
            "https://www.youtube.com/watch?v=abc123"
        );
        assert!(first_youtube_url(b"https://example.com/video\n").is_err());
        assert!(first_youtube_url(b"\n").is_err());
    }
}
