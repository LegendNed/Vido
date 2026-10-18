//! Microphone noise suppression (RNNoise via `nnnoiseless`, runs locally).
//!
//! RNNoise works on 480-sample mono frames at 48 kHz. Input is queued and the
//! output is delayed by exactly [`DENOISE_DELAY`] samples; the mixer reads
//! denoised sources that much earlier so they stay aligned with video.

use std::collections::VecDeque;

use nnnoiseless::DenoiseState;

pub const DENOISE_DELAY: usize = DenoiseState::FRAME_SIZE;

pub struct Denoiser {
    state: Box<DenoiseState<'static>>,
    input: Vec<f32>,
    output: VecDeque<f32>,
    frame_out: Vec<f32>,
}

impl Denoiser {
    pub fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            input: Vec::with_capacity(DenoiseState::FRAME_SIZE * 2),
            output: std::iter::repeat_n(0.0, DENOISE_DELAY).collect(),
            frame_out: vec![0.0; DenoiseState::FRAME_SIZE],
        }
    }

    /// Denoises interleaved stereo in place (downmixed to mono, then duplicated).
    pub fn process_stereo(&mut self, buf: &mut [f32]) {
        for frame in buf.chunks_exact(2) {
            // RNNoise expects 16-bit scale.
            self.input.push((frame[0] + frame[1]) * 0.5 * 32768.0);
        }
        let fs = DenoiseState::FRAME_SIZE;
        let mut consumed = 0;
        while self.input.len() - consumed >= fs {
            self.state
                .process_frame(&mut self.frame_out, &self.input[consumed..consumed + fs]);
            self.output
                .extend(self.frame_out.iter().map(|s| s / 32768.0));
            consumed += fs;
        }
        self.input.drain(..consumed);
        for frame in buf.chunks_exact_mut(2) {
            let s = self.output.pop_front().unwrap_or(0.0);
            frame[0] = s;
            frame[1] = s;
        }
    }
}

impl Default for Denoiser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_length_and_constant_delay() {
        let mut d = Denoiser::new();
        // Arbitrary block sizes (AAC frames are 1024).
        let mut total_out = 0;
        for n in [1024usize, 1024, 777, 1024, 3] {
            let mut buf = vec![0.1f32; n * 2];
            d.process_stereo(&mut buf);
            total_out += n;
            assert_eq!(buf.len(), n * 2);
        }
        assert_eq!(total_out, 1024 * 3 + 777 + 3);
        // Queued input never exceeds one RNNoise frame.
        assert!(d.input.len() < DenoiseState::FRAME_SIZE);
    }

    #[test]
    fn attenuates_white_noise() {
        let mut d = Denoiser::new();
        let mut seed = 12345u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            ((seed >> 16) as f32 / 32768.0 - 1.0) * 0.05
        };
        let mut energy_in = 0.0;
        let mut energy_out = 0.0;
        for i in 0..200 {
            let mut buf: Vec<f32> = (0..1024 * 2).map(|_| noise()).collect();
            let e_in: f32 = buf.iter().map(|s| s * s).sum();
            d.process_stereo(&mut buf);
            if i > 50 {
                energy_in += e_in;
                energy_out += buf.iter().map(|s| s * s).sum::<f32>();
            }
        }
        assert!(
            energy_out < energy_in * 0.5,
            "in={energy_in} out={energy_out}"
        );
    }
}
