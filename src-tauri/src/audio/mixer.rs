//! Sample-accurate source timelines and mixing (pure logic, unit-tested).
//!
//! Every capture packet carries a QPC timestamp. It is mapped to a sample index
//! on the session's media timeline. Small jitter is ignored (packets are simply
//! appended), real gaps are filled with silence (loopback delivers nothing while
//! nothing plays), and overlaps (clock drift) are trimmed.

use std::collections::VecDeque;

/// Stereo interleaved.
pub const CHANNELS: usize = 2;

pub struct SourceTimeline {
    /// Sample index of the first frame in `buf`.
    start: i64,
    buf: VecDeque<f32>,
    /// Tolerance before a timestamp mismatch is treated as a gap/overlap.
    tolerance: i64,
    started: bool,
}

impl SourceTimeline {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            start: 0,
            buf: VecDeque::new(),
            // 20 ms
            tolerance: i64::from(sample_rate) / 50,
            started: false,
        }
    }

    fn frames(&self) -> i64 {
        (self.buf.len() / CHANNELS) as i64
    }

    /// Sample index just after the last buffered frame.
    pub fn end(&self) -> i64 {
        self.start + self.frames()
    }

    /// Inserts `samples` (interleaved stereo) whose first frame belongs at `pos`.
    pub fn insert(&mut self, pos: i64, samples: &[f32]) {
        let n = (samples.len() / CHANNELS) as i64;
        if n == 0 {
            return;
        }
        if !self.started {
            self.started = true;
            self.start = pos;
            self.buf.extend(samples);
            return;
        }
        let expected = self.end();
        let delta = pos - expected;
        if delta.abs() <= self.tolerance {
            // Contiguous within jitter: keep the stream continuous.
            self.buf.extend(samples);
        } else if delta > 0 {
            // Gap: silence in between.
            self.buf
                .extend(std::iter::repeat_n(0.0, delta as usize * CHANNELS));
            self.buf.extend(samples);
        } else {
            // Overlap: drop what we already have.
            let skip = (-delta).min(n) as usize;
            self.buf.extend(&samples[skip * CHANNELS..]);
        }
    }

    /// Fills `out` (n frames, interleaved) with the range `[from, from + n)`;
    /// missing frames are silence. Data before `from + n` is discarded.
    pub fn take(&mut self, from: i64, out: &mut [f32]) {
        let n = (out.len() / CHANNELS) as i64;
        out.fill(0.0);
        // Discard anything older than `from`.
        if self.start < from {
            let drop = (from - self.start).min(self.frames());
            self.buf.drain(..drop as usize * CHANNELS);
            self.start += drop;
            if self.buf.is_empty() {
                self.start = self.start.max(from);
            }
        }
        // Copy the overlapping part.
        let offset = (self.start - from).max(0);
        if offset < n {
            let avail = self.frames().min(n - offset);
            let dst = &mut out[offset as usize * CHANNELS..(offset + avail) as usize * CHANNELS];
            for (d, s) in dst
                .iter_mut()
                .zip(self.buf.drain(..avail as usize * CHANNELS))
            {
                *d = s;
            }
            self.start += avail;
        }
        if self.buf.is_empty() && self.start < from + n {
            self.start = from + n;
        }
    }

    /// Drops everything (e.g. after a device reconnect).
    pub fn reset(&mut self) {
        self.buf.clear();
        self.started = false;
    }
}

/// Per-source processing parameters (can change live).
#[derive(Debug, Clone, Copy)]
pub struct TrackParams {
    pub gain: f32,
    pub muted: bool,
    pub mono: bool,
}

impl TrackParams {
    pub fn new(volume: f32, gain_db: f32, muted: bool, mono: bool) -> Self {
        Self {
            gain: volume * 10f32.powf(gain_db / 20.0),
            muted,
            mono,
        }
    }
}

/// Applies gain/mute/mono in place and returns (peak, rms) after processing.
pub fn process(buf: &mut [f32], p: TrackParams) -> (f32, f32) {
    let gain = if p.muted { 0.0 } else { p.gain };
    let mut peak = 0f32;
    let mut sum = 0f64;
    for frame in buf.chunks_exact_mut(CHANNELS) {
        if p.mono {
            let m = (frame[0] + frame[1]) * 0.5;
            frame[0] = m;
            frame[1] = m;
        }
        for s in frame.iter_mut() {
            *s *= gain;
            peak = peak.max(s.abs());
            sum += f64::from(*s) * f64::from(*s);
        }
    }
    let rms = if buf.is_empty() {
        0.0
    } else {
        (sum / buf.len() as f64).sqrt() as f32
    };
    (peak, rms)
}

/// Sums sources into `out` with a hard limit to [-1, 1].
pub fn mix_into(out: &mut [f32], sources: &[&[f32]]) {
    out.fill(0.0);
    for src in sources {
        for (o, s) in out.iter_mut().zip(src.iter()) {
            *o += *s;
        }
    }
    for o in out.iter_mut() {
        *o = o.clamp(-1.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo(vals: &[f32]) -> Vec<f32> {
        vals.iter().flat_map(|v| [*v, *v]).collect()
    }

    fn mono_of(buf: &[f32]) -> Vec<f32> {
        buf.chunks_exact(2).map(|c| c[0]).collect()
    }

    #[test]
    fn contiguous_inserts_and_takes() {
        let mut t = SourceTimeline::new(48_000);
        t.insert(100, &stereo(&[1.0, 2.0, 3.0]));
        t.insert(103, &stereo(&[4.0, 5.0]));
        let mut out = vec![0.0; 2 * 4];
        t.take(100, &mut out);
        assert_eq!(mono_of(&out), [1.0, 2.0, 3.0, 4.0]);
        let mut out = vec![0.0; 2 * 3];
        t.take(104, &mut out);
        assert_eq!(mono_of(&out), [5.0, 0.0, 0.0]);
    }

    #[test]
    fn jitter_within_tolerance_is_ignored() {
        let mut t = SourceTimeline::new(48_000); // tolerance 960
        t.insert(0, &stereo(&[1.0; 10]));
        t.insert(15, &stereo(&[2.0; 10])); // 5 frames "late" → appended at 10
        assert_eq!(t.end(), 20);
    }

    #[test]
    fn gaps_are_filled_with_silence() {
        let mut t = SourceTimeline::new(1000); // tolerance 20
        t.insert(0, &stereo(&[1.0; 5]));
        t.insert(105, &stereo(&[2.0; 5]));
        let mut out = vec![0.0; 2 * 110];
        t.take(0, &mut out);
        let m = mono_of(&out);
        assert_eq!(&m[..5], &[1.0; 5]);
        assert!(m[5..105].iter().all(|v| *v == 0.0));
        assert_eq!(&m[105..110], &[2.0; 5]);
    }

    #[test]
    fn overlaps_are_trimmed() {
        let mut t = SourceTimeline::new(1000);
        t.insert(0, &stereo(&[1.0; 100]));
        t.insert(50, &stereo(&[2.0; 100])); // 50 frames overlap → only last 50 appended
        assert_eq!(t.end(), 150);
        let mut out = vec![0.0; 2 * 150];
        t.take(0, &mut out);
        let m = mono_of(&out);
        assert_eq!(m[99], 1.0);
        assert_eq!(m[100], 2.0);
    }

    #[test]
    fn take_before_data_yields_leading_silence() {
        let mut t = SourceTimeline::new(48_000);
        t.insert(10, &stereo(&[7.0; 4]));
        let mut out = vec![0.0; 2 * 8];
        t.take(8, &mut out);
        assert_eq!(mono_of(&out), [0.0, 0.0, 7.0, 7.0, 7.0, 7.0, 0.0, 0.0]);
        // Data far older than the consumed range (beyond the jitter tolerance) is dropped.
        t.insert(16 - 2_000, &stereo(&[9.0; 2]));
        let mut out = vec![0.0; 2 * 2];
        t.take(16, &mut out);
        assert_eq!(mono_of(&out), [0.0, 0.0]);
    }

    #[test]
    fn processing_applies_gain_mute_and_mono() {
        let mut b = vec![1.0, 0.0, 0.5, -0.5];
        let (peak, _) = process(&mut b, TrackParams::new(0.5, 0.0, false, true));
        assert_eq!(b, [0.25, 0.25, 0.0, 0.0]);
        assert_eq!(peak, 0.25);
        let mut b = vec![1.0, 1.0];
        process(&mut b, TrackParams::new(1.0, 6.0206, false, false));
        assert!((b[0] - 2.0).abs() < 1e-3);
        let mut b = vec![1.0, 1.0];
        assert_eq!(
            process(&mut b, TrackParams::new(1.0, 0.0, true, false)).0,
            0.0
        );
    }

    #[test]
    fn mixing_sums_and_limits() {
        let a = [0.6f32, -0.2];
        let b = [0.6f32, -0.2];
        let mut out = [0.0; 2];
        mix_into(&mut out, &[&a, &b]);
        assert_eq!(out, [1.0, -0.4]);
    }
}
