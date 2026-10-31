//! Streaming mono PCM conversion for the fixed-rate virtual microphone.
pub const MICROPHONE_RATE: u32 = 48_000;

#[derive(Default)]
pub struct PcmResampler {
    input_rate: u32,
    previous: Option<i16>,
    input_time: u64,
    next_output: u64,
}

impl PcmResampler {
    pub fn convert(&mut self, pcm: &[u8], input_rate: u32) -> Vec<u8> {
        assert!(input_rate > 0);
        if self.input_rate != input_rate {
            *self = Self {
                input_rate,
                ..Self::default()
            };
        }
        let mut output =
            Vec::with_capacity(pcm.len() * MICROPHONE_RATE as usize / input_rate as usize + 4);
        for bytes in pcm.chunks_exact(2) {
            let sample = i16::from_le_bytes([bytes[0], bytes[1]]);
            if let Some(previous) = self.previous {
                let end = self.input_time + u64::from(MICROPHONE_RATE);
                while self.next_output <= end {
                    let fraction =
                        (self.next_output - self.input_time) as f64 / f64::from(MICROPHONE_RATE);
                    let value = (f64::from(previous)
                        + (f64::from(sample) - f64::from(previous)) * fraction)
                        .round() as i16;
                    output.extend_from_slice(&value.to_le_bytes());
                    self.next_output += u64::from(input_rate);
                }
                self.input_time = end;
            } else {
                output.extend_from_slice(&sample.to_le_bytes());
                self.next_output = u64::from(input_rate);
            }
            self.previous = Some(sample);
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }
    #[test]
    fn native_rate_is_bit_exact_across_packet_boundaries() {
        let samples = [-32768, -12000, -1, 0, 1, 15000, 32767];
        let mut resampler = PcmResampler::default();
        let mut actual = resampler.convert(&bytes(&samples[..3]), 48000);
        actual.extend(resampler.convert(&bytes(&samples[3..]), 48000));
        assert_eq!(actual, bytes(&samples));
    }
    #[test]
    fn phone_44100_conversion_is_continuous_and_preserves_duration() {
        let samples: Vec<i16> = (0..44100)
            .map(|i| {
                ((i as f64 * 440.0 * 2.0 * std::f64::consts::PI / 44100.0).sin() * 12000.0) as i16
            })
            .collect();
        let pcm = bytes(&samples);
        let reference = PcmResampler::default().convert(&pcm, 44100);
        let mut chunked = PcmResampler::default();
        let actual: Vec<u8> = pcm
            .chunks(882)
            .flat_map(|chunk| chunked.convert(chunk, 44100))
            .collect();
        assert_eq!(actual, reference);
        assert!((actual.len() as i64 / 2 - 48000).abs() <= 2);
        assert!(actual
            .chunks_exact(2)
            .all(|b| i16::from_le_bytes([b[0], b[1]]).abs() <= 12000));
    }
    #[test]
    fn rate_changes_reset_interpolation_without_old_samples() {
        let mut resampler = PcmResampler::default();
        resampler.convert(&bytes(&[30000; 20]), 44100);
        assert_eq!(resampler.convert(&bytes(&[0; 20]), 48000), bytes(&[0; 20]));
    }
}
