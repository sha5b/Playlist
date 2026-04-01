use std::path::Path;
use std::process::{Command, Stdio};

/// Extensions that rodio/symphonia cannot decode natively.
const TRANSCODE_EXTENSIONS: &[&str] = &["opus", "wma", "ape", "wv"];

/// Check if a file extension requires transcoding via ffmpeg.
pub fn needs_transcode(file_path: &str) -> bool {
    Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| TRANSCODE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Transcode an audio file to WAV using ffmpeg, returning the WAV bytes.
/// Uses stdout pipe (no temp files). Returns Err if ffmpeg fails.
///
/// ffmpeg can't seek back on a pipe, so RIFF/data chunk sizes in the header
/// are written as 0. We patch them after receiving all bytes.
pub fn transcode_to_wav(file_path: &str, ffmpeg_path: &str) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new(ffmpeg_path);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // IDLE_PRIORITY_CLASS | CREATE_NO_WINDOW
        cmd.creation_flags(0x00000040 | 0x08000000);
    }

    cmd.args([
        "-i", file_path,
        "-f", "wav",
        "-acodec", "pcm_s16le",
        "-loglevel", "error",
        "-",
    ]);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run ffmpeg: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg transcode failed: {}", stderr.trim()));
    }

    let mut wav = output.stdout;
    fix_wav_header(&mut wav);
    Ok(wav)
}

/// Patch RIFF and data chunk sizes in a WAV header.
/// When ffmpeg pipes to stdout it can't seek back to fill these in,
/// so they are typically 0 — which makes symphonia think there are no samples.
fn fix_wav_header(wav: &mut [u8]) {
    let file_size = wav.len();
    if file_size < 12 {
        return;
    }
    // Fix RIFF chunk size at offset 4: (file_size - 8)
    let riff_size = (file_size - 8) as u32;
    wav[4..8].copy_from_slice(&riff_size.to_le_bytes());

    // Find "data" subchunk and fix its size
    if let Some(pos) = wav.windows(4).position(|w| w == b"data") {
        if pos + 8 <= file_size {
            let data_size = (file_size - pos - 8) as u32;
            wav[pos + 4..pos + 8].copy_from_slice(&data_size.to_le_bytes());
        }
    }
}
