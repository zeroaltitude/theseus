//! The system tools a file's reading uses when they are present
//! (theseus-c9l6): ffprobe for a recording's length, ffmpeg for a video's
//! audio track and a strip of its frames, and tesseract for the text in a
//! picture. Each runs under the converter's caps (`convert::run_capped`),
//! with no file it may write; one that is absent is said, never a failure.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::convert::{self, Limits};

/// Where a tool may be besides the daemon's `PATH`: a user service's `PATH`
/// is short, and Homebrew's prefix is common.
const MORE_DIRS: &[&str] = &[
    "/home/linuxbrew/.linuxbrew/bin",
    "/usr/local/bin",
    "/usr/bin",
    "/opt/homebrew/bin",
];

/// A tool's path, from `PATH` or the usual places; `None` when it is absent.
pub fn which(tool: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(MORE_DIRS.iter().map(PathBuf::from))
        .map(|d| d.join(tool))
        .find(|p| p.is_file())
}

/// The limits a media tool runs under: more time and memory than a document,
/// since decoding video is real work.
pub const MEDIA_LIMITS: Limits = Limits {
    timeout: Duration::from_secs(90),
    memory_bytes: 2 * 1024 * 1024 * 1024,
};

fn tool(name: &str) -> Result<PathBuf, String> {
    which(name).ok_or_else(|| format!("{name} is not installed on this machine"))
}

/// A recording's length in seconds (ffprobe), when ffprobe is present and
/// can read it.
pub fn duration(path: &Path) -> Option<f64> {
    let mut cmd = Command::new(which("ffprobe")?);
    cmd.env_clear();
    cmd.args([
        "-v",
        "error",
        "-show_entries",
        "format=duration",
        "-of",
        "csv=p=0",
    ])
    .arg(path);
    let out = convert::run_capped(&mut cmd, &[], MEDIA_LIMITS).ok()?;
    String::from_utf8_lossy(&out)
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|d| d.is_finite() && *d >= 0.0)
}

/// A recording's audio, as 16 kHz mono WAV (ffmpeg): what speech to text
/// reads of a video.
pub fn audio_of(path: &Path) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new(tool("ffmpeg")?);
    cmd.env_clear();
    cmd.args(["-nostdin", "-v", "error", "-threads", "2", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "wav", "pipe:1"]);
    let out = convert::run_capped(&mut cmd, &[], MEDIA_LIMITS)
        .map_err(|e| format!("ffmpeg could not take its audio: {e}"))?;
    if out.len() <= 44 {
        return Err("it has no audio track".into());
    }
    Ok(out)
}

/// A recording's first `seconds` of audio, as `audio_of` gives it: a long
/// recording is heard only to its cap.
pub fn audio_of_first(path: &Path, seconds: f64) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new(tool("ffmpeg")?);
    cmd.env_clear();
    cmd.args(["-nostdin", "-v", "error", "-threads", "2", "-i"])
        .arg(path)
        .args(["-t", &format!("{seconds:.0}")])
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "wav", "pipe:1"]);
    let out = convert::run_capped(&mut cmd, &[], MEDIA_LIMITS)
        .map_err(|e| format!("ffmpeg could not take its audio: {e}"))?;
    if out.len() <= 44 {
        return Err("it has no audio track".into());
    }
    Ok(out)
}

/// A strip of `n` frames spread across a video, side by side in one PNG
/// (ffmpeg's `tile`), so one image shows it; `seconds` is its length.
pub fn frames(path: &Path, n: u32, seconds: f64) -> Result<Vec<u8>, String> {
    let n = n.clamp(1, 6);
    let every = (seconds / f64::from(n)).max(0.5);
    let filter = format!("fps=1/{every:.3},scale=480:-2,tile={n}x1");
    let mut cmd = Command::new(tool("ffmpeg")?);
    cmd.env_clear();
    cmd.args(["-nostdin", "-v", "error", "-threads", "2", "-i"])
        .arg(path)
        .args([
            "-vf",
            &filter,
            "-frames:v",
            "1",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "pipe:1",
        ]);
    let out = convert::run_capped(&mut cmd, &[], MEDIA_LIMITS)
        .map_err(|e| format!("ffmpeg could not take its frames: {e}"))?;
    if !out.starts_with(b"\x89PNG") {
        return Err("ffmpeg made no frames of it".into());
    }
    Ok(out)
}

/// The text in a picture (tesseract, English), when tesseract is present.
pub fn ocr(image: &[u8]) -> Result<String, String> {
    let mut cmd = Command::new(tool("tesseract")?);
    cmd.env_clear();
    cmd.args(["stdin", "stdout", "-l", "eng"]);
    let out = convert::run_capped(&mut cmd, image, MEDIA_LIMITS)
        .map_err(|e| format!("tesseract could not read it: {e}"))?;
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_tool_is_said_not_failed() {
        assert!(which("theseus-no-such-tool").is_none());
        assert_eq!(
            tool("theseus-no-such-tool").unwrap_err(),
            "theseus-no-such-tool is not installed on this machine"
        );
        assert!(duration(Path::new("/nonexistent/clip.mp4")).is_none());
    }
}
