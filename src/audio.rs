//! Volume scaling for the embedded 16-bit PCM alert sounds.
//!
//! `PlaySoundW` has no volume parameter, so the host plays a scaled copy of the WAV. Every
//! sound is normalised to the same peak before the volume curve is applied, so switching
//! sounds never causes a sudden jump in loudness, and the result can never clip.

/// Peak level (fraction of full scale) a sound reaches at 100% volume.
pub const MAX_PEAK: f32 = 0.95;

/// Target peak for a 0–100 volume. A squared curve tracks perceived loudness far better
/// than a linear one, so each slider step sounds roughly even.
pub fn volume_peak(volume: u8) -> f32 {
    let v = f32::from(volume.min(100)) / 100.0;
    MAX_PEAK * v * v
}

/// Returns a copy of `wav` scaled to `volume`, or `None` when the data is not a PCM16 WAV
/// this module understands (the caller then plays the original unchanged).
pub fn scale_wav(wav: &[u8], volume: u8) -> Option<Vec<u8>> {
    let (start, len) = pcm16_data(wav)?;
    let mut out = wav.to_vec();
    let samples = &mut out[start..start + len];
    let peak = samples
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs())
        .max()
        .unwrap_or(0);
    let target = volume_peak(volume);
    let gain = if peak == 0 {
        0.0
    } else {
        target * 32767.0 / f32::from(peak)
    };
    for b in samples.chunks_exact_mut(2) {
        let s = f32::from(i16::from_le_bytes([b[0], b[1]])) * gain;
        let s = s.round().clamp(-32767.0, 32767.0) as i16;
        b.copy_from_slice(&s.to_le_bytes());
    }
    Some(out)
}

/// Peak of a PCM16 WAV as a fraction of full scale.
pub fn wav_peak(wav: &[u8]) -> Option<f32> {
    let (start, len) = pcm16_data(wav)?;
    let peak = wav[start..start + len]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs())
        .max()
        .unwrap_or(0);
    Some(f32::from(peak) / 32767.0)
}

/// Locate the sample bytes of a little-endian RIFF/WAVE file with 16-bit PCM samples.
fn pcm16_data(wav: &[u8]) -> Option<(usize, usize)> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12;
    let mut pcm16 = false;
    while pos + 8 <= wav.len() {
        let id = &wav[pos..pos + 4];
        let size = u32::from_le_bytes(wav[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = pos + 8;
        let end = body.checked_add(size)?;
        if id == b"fmt " {
            let fmt = wav.get(body..body + 16)?;
            let format = u16::from_le_bytes([fmt[0], fmt[1]]);
            let bits = u16::from_le_bytes([fmt[14], fmt[15]]);
            pcm16 = format == 1 && bits == 16;
        } else if id == b"data" {
            if !pcm16 {
                return None;
            }
            let len = size.min(wav.len().saturating_sub(body)) & !1;
            return Some((body, len));
        }
        pos = end + (size & 1);
    }
    None
}
