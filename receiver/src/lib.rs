pub const PACKET_MAGIC: &[u8; 4] = b"VXL1";
pub const PACKET_HEADER_LEN: usize = 12;

#[derive(Clone, Copy, Debug)]
pub struct AudioPacket<'a> {
    pub sequence: u32,
    pub sample_rate: u32,
    pub pcm: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketError {
    TooShort,
    InvalidMagic,
    EmptyPayload,
    MisalignedPcm,
    UnsupportedSampleRate,
}

impl AudioPacket<'_> {
    pub fn parse(bytes: &[u8]) -> Result<AudioPacket<'_>, PacketError> {
        if bytes.len() < PACKET_HEADER_LEN {
            return Err(PacketError::TooShort);
        }
        if &bytes[..4] != PACKET_MAGIC {
            return Err(PacketError::InvalidMagic);
        }

        let pcm = &bytes[PACKET_HEADER_LEN..];
        if pcm.is_empty() {
            return Err(PacketError::EmptyPayload);
        }
        if pcm.len() % 2 != 0 {
            return Err(PacketError::MisalignedPcm);
        }

        let sequence = u32::from_be_bytes(bytes[4..8].try_into().expect("fixed-size slice"));
        let sample_rate = u32::from_be_bytes(bytes[8..12].try_into().expect("fixed-size slice"));
        if !(8_000..=192_000).contains(&sample_rate) {
            return Err(PacketError::UnsupportedSampleRate);
        }

        Ok(AudioPacket {
            sequence,
            sample_rate,
            pcm,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioPacket, PacketError};

    #[test]
    fn parses_vxl1_header_and_pcm_payload() {
        let bytes = [*b"VXL1", 0, 0, 0, 7, 0, 0, 0, 0xBB, 0x80, 0x34, 0x12];
        let packet = AudioPacket::parse(&bytes).unwrap();
        assert_eq!(packet.sequence, 7);
        assert_eq!(packet.sample_rate, 48_000);
        assert_eq!(packet.pcm, &[0x34, 0x12]);
    }

    #[test]
    fn rejects_malformed_audio_packets() {
        assert_eq!(AudioPacket::parse(b"VXL"), Err(PacketError::TooShort));
        assert_eq!(AudioPacket::parse(b"NOPE\0\0\0\0\0\0\xBB\x80\0\0"), Err(PacketError::InvalidMagic));
        assert_eq!(AudioPacket::parse(b"VXL1\0\0\0\0\0\0\xBB\x80"), Err(PacketError::EmptyPayload));
        assert_eq!(AudioPacket::parse(b"VXL1\0\0\0\0\0\0\xBB\x80\0"), Err(PacketError::MisalignedPcm));
        assert_eq!(AudioPacket::parse(b"VXL1\0\0\0\0\0\0\0\0\0\0"), Err(PacketError::UnsupportedSampleRate));
    }
}
