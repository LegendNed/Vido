//! Session clock shared by video and audio.
//!
//! All capture timestamps are converted to **media time**: microseconds since
//! the session started, excluding time spent paused. Video ticks and WASAPI
//! packets both carry QPC-derived timestamps (100 ns units), so mapping both
//! through the same clock keeps them in sync.

use parking_lot::Mutex;

/// 100-nanosecond units (the unit of WASAPI QPC positions and WinRT TimeSpan).
pub type Hns = i64;

pub const HNS_PER_SEC: i64 = 10_000_000;
pub const HNS_PER_US: i64 = 10;

/// Current QPC time in 100 ns units.
#[cfg(windows)]
pub fn now_hns() -> Hns {
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    static FREQ: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    let freq = *FREQ.get_or_init(|| {
        let mut f = 0i64;
        // SAFETY: valid out pointer.
        unsafe { QueryPerformanceFrequency(&mut f).ok() };
        f.max(1)
    });
    let mut c = 0i64;
    // SAFETY: valid out pointer.
    unsafe { QueryPerformanceCounter(&mut c).ok() };
    (i128::from(c) * i128::from(HNS_PER_SEC) / i128::from(freq)) as i64
}

#[cfg(not(windows))]
pub fn now_hns() -> Hns {
    use std::time::Instant;
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let s = *START.get_or_init(Instant::now);
    (s.elapsed().as_nanos() / 100) as i64
}

#[derive(Debug, Default)]
struct PauseState {
    paused_at: Option<Hns>,
    /// Completed pause intervals `[begin, end)`, in QPC hns.
    intervals: Vec<(Hns, Hns)>,
}

#[derive(Debug)]
pub struct SessionClock {
    start: Hns,
    pauses: Mutex<PauseState>,
}

impl SessionClock {
    pub fn new() -> Self {
        Self::starting_at(now_hns())
    }

    pub fn starting_at(start: Hns) -> Self {
        Self {
            start,
            pauses: Mutex::new(PauseState::default()),
        }
    }

    pub fn start_hns(&self) -> Hns {
        self.start
    }

    pub fn pause_at(&self, at: Hns) {
        let mut p = self.pauses.lock();
        if p.paused_at.is_none() {
            p.paused_at = Some(at);
        }
    }

    pub fn resume_at(&self, at: Hns) {
        let mut p = self.pauses.lock();
        if let Some(begin) = p.paused_at.take() {
            if at > begin {
                p.intervals.push((begin, at));
            }
        }
    }

    pub fn pause(&self) {
        self.pause_at(now_hns());
    }

    pub fn resume(&self) {
        self.resume_at(now_hns());
    }

    pub fn is_paused(&self) -> bool {
        self.pauses.lock().paused_at.is_some()
    }

    /// Maps a QPC timestamp to media time (hns since start, pauses removed).
    /// Returns `None` if the timestamp falls inside a pause.
    pub fn media_hns(&self, qpc: Hns) -> Option<Hns> {
        let p = self.pauses.lock();
        if let Some(begin) = p.paused_at {
            if qpc >= begin {
                return None;
            }
        }
        let mut paused_before = 0;
        for &(b, e) in &p.intervals {
            if qpc >= e {
                paused_before += e - b;
            } else if qpc >= b {
                return None;
            }
        }
        Some(qpc - self.start - paused_before)
    }

    pub fn media_us(&self, qpc: Hns) -> Option<i64> {
        self.media_hns(qpc).map(|h| h.div_euclid(HNS_PER_US))
    }

    /// Media time now; while paused, the media time at which the pause began.
    pub fn media_now_hns(&self) -> Hns {
        let now = now_hns();
        let paused_at = self.pauses.lock().paused_at;
        match paused_at {
            Some(b) => self.media_hns(b - 1).unwrap_or(0),
            None => self.media_hns(now).unwrap_or(0),
        }
    }

    /// Converts a media time back to the QPC time it corresponds to (for
    /// scheduling), assuming no further pauses.
    pub fn qpc_for_media(&self, media: Hns) -> Hns {
        let p = self.pauses.lock();
        let paused: Hns = p.intervals.iter().map(|(b, e)| e - b).sum();
        self.start + media + paused
    }
}

impl Default for SessionClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Converts a sample count at `rate` Hz to hns.
pub fn samples_to_hns(samples: i64, rate: u32) -> Hns {
    (i128::from(samples) * i128::from(HNS_PER_SEC) / i128::from(rate)) as i64
}

/// Converts hns to a sample index at `rate` Hz (rounded to nearest).
pub fn hns_to_samples(h: Hns, rate: u32) -> i64 {
    let num = i128::from(h) * i128::from(rate);
    let den = i128::from(HNS_PER_SEC);
    // Round half away from zero.
    let q = if num >= 0 {
        (num + den / 2) / den
    } else {
        (num - den / 2) / den
    };
    q as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Hns = HNS_PER_SEC;

    #[test]
    fn media_time_without_pauses() {
        let c = SessionClock::starting_at(100 * S);
        assert_eq!(c.media_hns(100 * S), Some(0));
        assert_eq!(c.media_hns(103 * S), Some(3 * S));
        // Slightly before start (audio captured before the session began).
        assert_eq!(c.media_hns(100 * S - 5), Some(-5));
        assert_eq!(c.media_us(101 * S), Some(1_000_000));
    }

    #[test]
    fn pauses_are_removed_from_timeline() {
        let c = SessionClock::starting_at(0);
        c.pause_at(10 * S);
        assert!(c.is_paused());
        assert_eq!(c.media_hns(11 * S), None);
        assert_eq!(c.media_hns(9 * S), Some(9 * S));
        c.resume_at(15 * S);
        assert!(!c.is_paused());
        // Inside the completed pause interval.
        assert_eq!(c.media_hns(12 * S), None);
        // After resume, media time continues where it stopped.
        assert_eq!(c.media_hns(15 * S), Some(10 * S));
        assert_eq!(c.media_hns(20 * S), Some(15 * S));
        // Second pause.
        c.pause_at(30 * S);
        c.resume_at(31 * S);
        assert_eq!(c.media_hns(32 * S), Some(26 * S));
        assert_eq!(c.qpc_for_media(26 * S), 32 * S);
    }

    #[test]
    fn video_and_audio_align_on_shared_clock() {
        // A video tick and an audio packet captured at the same QPC instant
        // must map to the same media time, even across a pause.
        let c = SessionClock::starting_at(5 * S);
        c.pause_at(7 * S);
        c.resume_at(9 * S);
        let qpc = 12 * S + 1234;
        let video_us = c.media_us(qpc).unwrap();
        let audio_samples = hns_to_samples(c.media_hns(qpc).unwrap(), 48_000);
        let audio_us = samples_to_hns(audio_samples, 48_000) / HNS_PER_US;
        assert!(
            (video_us - audio_us).abs() <= 21,
            "{video_us} vs {audio_us}"
        );
    }

    #[test]
    fn sample_conversion_round_trips() {
        for rate in [44_100u32, 48_000] {
            for s in [0i64, 1, 1023, 48_000, 1_234_567] {
                assert_eq!(hns_to_samples(samples_to_hns(s, rate), rate), s);
            }
        }
        assert_eq!(hns_to_samples(-S, 48_000), -48_000);
    }
}
