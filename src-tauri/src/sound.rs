//! The "clip saved" sound: a built-in WAV (see `scripts/synth-sounds.py`) or the
//! user's own file, scaled to the chosen volume and played asynchronously.

use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::settings::{GeneralSettings, SaveSound};

/// `config/save-sounds.json` (built in): the sound list and the default volume.
/// The labels are only used by the UI.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SoundConfig {
    default_volume: u32,
    /// Read by the UI; here only checked against the embedded files (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    sounds: Vec<SoundEntry>,
}

#[derive(Deserialize)]
#[cfg_attr(not(test), allow(dead_code))]
struct SoundEntry {
    id: SaveSound,
    /// File in `src-tauri/assets/sounds`; `None` for the custom sound.
    file: Option<String>,
}

fn config() -> &'static SoundConfig {
    static CONFIG: OnceLock<SoundConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        serde_json::from_str(include_str!("../../config/save-sounds.json"))
            .expect("config/save-sounds.json is valid (see tests)")
    })
}

/// Default save sound volume (percent).
pub fn default_volume() -> u32 {
    config().default_volume
}

const TAP: &[u8] = include_bytes!("../assets/sounds/tap.wav");
const TWO_TONE: &[u8] = include_bytes!("../assets/sounds/two-tone.wav");
const BLOOP: &[u8] = include_bytes!("../assets/sounds/bloop.wav");
const PLUCK: &[u8] = include_bytes!("../assets/sounds/pluck.wav");

/// Larger custom files are refused (the whole file is held in memory).
const MAX_CUSTOM_BYTES: u64 = 8 * 1024 * 1024;

/// Plays the save sound from `general` (when it is on). A custom file that can't
/// be played falls back to the default sound.
pub fn play_saved(general: &GeneralSettings) {
    if !general.sound_on_save {
        return;
    }
    let volume = general.save_sound_volume;
    if let Err(e) = play(
        general.save_sound,
        general.save_sound_path.as_deref(),
        volume,
    ) {
        tracing::warn!("save sound: {}", e.message);
        let _ = play(SaveSound::default(), None, volume);
    }
}

/// Plays `sound` (`path` for a custom one) at `volume` percent.
pub fn play(sound: SaveSound, path: Option<&str>, volume: u32) -> AppResult<()> {
    let mut wav = match built_in(sound) {
        Some(bytes) => bytes.to_vec(),
        None => read_custom(path)?,
    };
    scale_wav(&mut wav, volume.min(100) as f32 / 100.0)?;
    output::play(wav);
    Ok(())
}

/// The embedded WAV of a built-in sound (`None` for the custom one).
fn built_in(sound: SaveSound) -> Option<&'static [u8]> {
    match sound {
        SaveSound::Tap => Some(TAP),
        SaveSound::TwoTone => Some(TWO_TONE),
        SaveSound::Bloop => Some(BLOOP),
        SaveSound::Pluck => Some(PLUCK),
        SaveSound::Custom => None,
    }
}

fn read_custom(path: Option<&str>) -> AppResult<Vec<u8>> {
    let path = path
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| AppError::invalid("general.saveSoundPath", "Choose a WAV file"))?;
    let p = Path::new(path);
    let len = std::fs::metadata(p)
        .map_err(|e| AppError::invalid("general.saveSoundPath", format!("{path}: {e}")))?
        .len();
    if len > MAX_CUSTOM_BYTES {
        return Err(AppError::invalid(
            "general.saveSoundPath",
            "The sound file is larger than 8 MB",
        ));
    }
    std::fs::read(p).map_err(|e| AppError::invalid("general.saveSoundPath", format!("{path}: {e}")))
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Scales the samples of a RIFF/WAVE file in place (8/16/24/32-bit PCM and
/// 32-bit float, also as WAVE_FORMAT_EXTENSIBLE).
fn scale_wav(wav: &mut [u8], gain: f32) -> AppResult<()> {
    let bad = || AppError::invalid("general.saveSoundPath", "Not a supported WAV file");
    if wav.get(0..4) != Some(b"RIFF") || wav.get(8..12) != Some(b"WAVE") {
        return Err(bad());
    }
    let mut format = None;
    let mut i = 12;
    while i + 8 <= wav.len() {
        let id = &wav[i..i + 4];
        let size = u32_at(wav, i + 4).ok_or_else(bad)? as usize;
        let body = i + 8;
        let end = body.saturating_add(size).min(wav.len());
        if id == b"fmt " {
            let mut tag = u16_at(wav, body).ok_or_else(bad)?;
            let bits = u16_at(wav, body + 14).ok_or_else(bad)?;
            if tag == 0xFFFE {
                // Extensible: the sub-format GUID starts with the real tag.
                tag = u16_at(wav, body + 24).ok_or_else(bad)?;
            }
            format = Some((tag, bits));
        } else if id == b"data" {
            let (tag, bits) = format.ok_or_else(bad)?;
            scale_samples(&mut wav[body..end], tag, bits, gain).ok_or_else(bad)?;
            return Ok(());
        }
        // Chunks are padded to an even size.
        i = body.saturating_add(size + (size & 1));
    }
    Err(bad())
}

fn scale_samples(data: &mut [u8], tag: u16, bits: u16, gain: f32) -> Option<()> {
    match (tag, bits) {
        (1, 8) => data.iter_mut().for_each(|b| {
            *b = ((*b as f32 - 128.0) * gain + 128.0)
                .round()
                .clamp(0.0, 255.0) as u8;
        }),
        (1, 16) => data.chunks_exact_mut(2).for_each(|c| {
            let v = i16::from_le_bytes([c[0], c[1]]) as f32 * gain;
            c.copy_from_slice(&(v.round() as i16).to_le_bytes());
        }),
        (1, 24) => data.chunks_exact_mut(3).for_each(|c| {
            let v = (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 * gain;
            let b = (v.round() as i32).to_le_bytes();
            c.copy_from_slice(&b[0..3]);
        }),
        (1, 32) => data.chunks_exact_mut(4).for_each(|c| {
            let v = i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f64 * gain as f64;
            c.copy_from_slice(&(v.round() as i32).to_le_bytes());
        }),
        (3, 32) => data.chunks_exact_mut(4).for_each(|c| {
            let v = f32::from_le_bytes([c[0], c[1], c[2], c[3]]) * gain;
            c.copy_from_slice(&v.to_le_bytes());
        }),
        _ => return None,
    }
    Some(())
}

#[cfg(windows)]
mod output {
    use parking_lot::Mutex;
    use windows::core::PCWSTR;
    use windows::Win32::Media::Audio::{
        PlaySoundW, SND_ASYNC, SND_FLAGS, SND_MEMORY, SND_NODEFAULT,
    };

    /// The sound being played: PlaySound reads it from memory while it plays.
    static CURRENT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

    pub fn play(wav: Vec<u8>) {
        let mut current = CURRENT.lock();
        // SAFETY: stop whatever plays from the old buffer before replacing it; the
        // new buffer stays in `CURRENT` until the next call.
        unsafe {
            let _ = PlaySoundW(PCWSTR::null(), None, SND_FLAGS(0));
            *current = wav;
            let _ = PlaySoundW(
                PCWSTR(current.as_ptr().cast()),
                None,
                SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
            );
        }
    }
}

#[cfg(not(windows))]
mod output {
    pub fn play(_wav: Vec<u8>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(wav: &[u8]) -> Vec<i16> {
        wav[44..]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect()
    }

    #[test]
    fn built_in_sounds_scale() {
        for wav in [TAP, TWO_TONE, BLOOP, PLUCK] {
            let mut half = wav.to_vec();
            scale_wav(&mut half, 0.5).unwrap();
            let (a, b) = (samples(wav), samples(&half));
            let peak = |s: &[i16]| s.iter().map(|v| v.unsigned_abs()).max().unwrap();
            assert!(peak(&a) > 10_000, "built-ins are about -9 dBFS");
            assert!((peak(&b) as i32 - peak(&a) as i32 / 2).abs() <= 1);
            let mut silent = wav.to_vec();
            scale_wav(&mut silent, 0.0).unwrap();
            assert!(samples(&silent).iter().all(|v| *v == 0));
        }
    }

    #[test]
    fn sound_config_matches_the_embedded_files() {
        let c = config();
        assert!(c.default_volume <= 100);
        let all = [
            SaveSound::Tap,
            SaveSound::TwoTone,
            SaveSound::Bloop,
            SaveSound::Pluck,
            SaveSound::Custom,
        ];
        assert_eq!(c.sounds.len(), all.len(), "one entry per sound");
        for s in all {
            let e = c.sounds.iter().find(|e| e.id == s).expect("listed");
            match (&e.file, built_in(s)) {
                (Some(f), Some(bytes)) => {
                    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/sounds");
                    assert_eq!(std::fs::read(dir.join(f)).unwrap(), bytes, "{f}");
                }
                (None, None) => {}
                _ => panic!("{s:?}: file and embedded sound disagree"),
            }
        }
    }

    #[test]
    fn other_files_are_refused() {
        assert!(scale_wav(&mut b"not a wav file at all".to_vec(), 1.0).is_err());
        let mut mp3ish = TAP.to_vec();
        mp3ish[20] = 0x55; // format tag: MPEG Layer 3
        assert!(scale_wav(&mut mp3ish, 1.0).is_err());
        assert!(play(SaveSound::Custom, None, 50).is_err());
        assert!(play(SaveSound::Custom, Some("C:\\nope\\missing.wav"), 50).is_err());
    }
}
