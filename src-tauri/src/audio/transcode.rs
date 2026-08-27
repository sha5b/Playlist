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

    // Walk the RIFF chunk list (id + u32 size, padded to even offsets) to find
    // the real "data" chunk. A raw windows() search would match the bytes
    // "data" inside a preceding LIST/INFO metadata chunk (e.g. a tag string
    // like "Database"), patch the wrong offset, and corrupt the stream.
    let mut pos = 12;
    while pos + 8 <= file_size {
        let chunk_id = &wav[pos..pos + 4];
        let size = u32::from_le_bytes([wav[pos + 4], wav[pos + 5], wav[pos + 6], wav[pos + 7]]) as usize;
        if chunk_id == b"data" {
            let data_size = (file_size - pos - 8) as u32;
            wav[pos + 4..pos + 8].copy_from_slice(&data_size.to_le_bytes());
            return;
        }
        // Chunks are word-aligned: odd sizes carry one pad byte
        pos += 8 + size + (size % 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_wav(list_payload: &[u8], pcm: &[u8]) -> Vec<u8> {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&0u32.to_le_bytes()); // zeroed: what ffmpeg pipes write
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"LIST");
        wav.extend_from_slice(&(list_payload.len() as u32).to_le_bytes());
        wav.extend_from_slice(list_payload);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&0u32.to_le_bytes());
        wav.extend_from_slice(pcm);
        wav
    }

    fn u32_at(wav: &[u8], off: usize) -> u32 {
        u32::from_le_bytes([wav[off], wav[off + 1], wav[off + 2], wav[off + 3]])
    }

    #[test]
    fn patches_data_chunk_after_list_containing_data_text() {
        // "data" inside tag text must not be mistaken for the data chunk
        let payload = b"INFOICMTmetadata database data";
        let pcm = [0u8; 100];
        let mut wav = build_wav(payload, &pcm);
        fix_wav_header(&mut wav);
        let data_off = 12 + 8 + payload.len();
        assert_eq!(u32_at(&wav, data_off + 4) as usize, pcm.len());
        assert_eq!(u32_at(&wav, 4) as usize, wav.len() - 8);
    }

    #[test]
    fn patches_plain_data_only_wav() {
        let pcm = [1u8, 2, 3, 4];
        let mut wav = build_wav(b"", &pcm);
        fix_wav_header(&mut wav);
        // empty LIST payload → data chunk starts right after its 8-byte header
        let data_off = 12 + 8;
        assert_eq!(u32_at(&wav, data_off + 4) as usize, pcm.len());
    }
}
