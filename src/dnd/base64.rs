//! Base64 as the drag-and-drop protocol uses it: padding optional, streams split anywhere.
//!
//! A sender encodes each message on its own, and may split the encoding across escape codes at
//! any byte. [`Decoder`] carries a partial group from one escape code to the next and is finished
//! at the end of each message, where an unpadded tail is complete. [`StreamEncoder`] produces the
//! opposite: only whole three-byte groups until the stream ends, so a receiver that decodes the
//! whole stream as one encoding reads it correctly too.

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

const LENIENT: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// Base64 that could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid base64 in drag-and-drop payload")]
pub struct InvalidBase64;

/// Append the unpadded encoding of `bytes`.
pub(crate) fn encode_into(bytes: &[u8], out: &mut Vec<u8>) {
    let start = out.len();
    out.resize(start + bytes.len().div_ceil(3) * 4, 0);
    let written = STANDARD_NO_PAD
        .encode_slice(bytes, &mut out[start..])
        .expect("buffer sized for the encoding");
    out.truncate(start + written);
}

/// Decodes one message's base64 as its escape codes arrive.
#[derive(Clone, Debug, Default)]
pub struct Decoder {
    pending: Vec<u8>,
}

impl Decoder {
    /// Decode what `input` completes, keeping a partial group for the next call.
    pub fn push(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), InvalidBase64> {
        for &byte in input {
            if matches!(byte, b'\r' | b'\n' | b' ' | b'\t') {
                continue;
            }
            self.pending.push(byte);
            if self.pending.len() == 4 {
                self.flush_group(out)?;
            }
        }
        Ok(())
    }

    /// End the message: decode a trailing unpadded group.
    pub fn finish(&mut self, out: &mut Vec<u8>) -> Result<(), InvalidBase64> {
        match self.pending.len() {
            0 => Ok(()),
            1 => {
                self.pending.clear();
                Err(InvalidBase64)
            }
            _ => self.flush_group(out),
        }
    }

    fn flush_group(&mut self, out: &mut Vec<u8>) -> Result<(), InvalidBase64> {
        let group = std::mem::take(&mut self.pending);
        let data: Vec<u8> = group.iter().copied().take_while(|&b| b != b'=').collect();
        if data.len() < 2 && !data.is_empty() || group[data.len()..].iter().any(|&b| b != b'=') {
            return Err(InvalidBase64);
        }
        if data.is_empty() {
            return Ok(());
        }
        let mut buffer = [0u8; 3];
        let written = LENIENT
            .decode_slice(&data, &mut buffer)
            .map_err(|_| InvalidBase64)?;
        out.extend_from_slice(&buffer[..written]);
        Ok(())
    }
}

/// Decode a whole message's payload.
#[cfg(test)]
pub fn decode(input: &[u8]) -> Result<Vec<u8>, InvalidBase64> {
    let mut decoder = Decoder::default();
    let mut out = Vec::with_capacity(input.len() / 4 * 3 + 3);
    decoder.push(input, &mut out)?;
    decoder.finish(&mut out)?;
    Ok(out)
}

/// Holds back a partial three-byte group until more data or the end of the stream arrives.
#[derive(Clone, Debug, Default)]
pub struct StreamEncoder {
    carry: Vec<u8>,
}

impl StreamEncoder {
    /// Take `bytes`, returning the longest prefix of everything taken so far that is a whole
    /// number of groups. The rest waits for the next call or [`Self::finish`].
    pub fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.carry.extend_from_slice(bytes);
        let whole = self.carry.len() / 3 * 3;
        let rest = self.carry.split_off(whole);
        std::mem::replace(&mut self.carry, rest)
    }

    /// Everything still held back.
    pub fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.carry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_split_at_every_byte_with_or_without_padding() {
        for data in [&b""[..], b"a", b"ab", b"abc", b"abcd", b"hello, world"] {
            for padded in [false, true] {
                let encoded = if padded {
                    base64::engine::general_purpose::STANDARD.encode(data)
                } else {
                    STANDARD_NO_PAD.encode(data)
                };
                for split in 0..=encoded.len() {
                    let mut decoder = Decoder::default();
                    let mut out = Vec::new();
                    decoder
                        .push(&encoded.as_bytes()[..split], &mut out)
                        .unwrap();
                    decoder
                        .push(&encoded.as_bytes()[split..], &mut out)
                        .unwrap();
                    decoder.finish(&mut out).unwrap();
                    assert_eq!(out, data, "{encoded:?} split at {split}");
                }
            }
        }
    }

    #[test]
    fn refuses_a_lone_character_and_garbage() {
        assert_eq!(decode(b"QUJD="), Err(InvalidBase64));
        assert_eq!(decode(b"Q"), Err(InvalidBase64));
        assert_eq!(decode(b"QU!D"), Err(InvalidBase64));
        assert_eq!(decode(b"Q=JD"), Err(InvalidBase64));
    }

    #[test]
    fn the_stream_encoder_emits_whole_groups_until_the_end() {
        let mut encoder = StreamEncoder::default();
        assert_eq!(encoder.push(b"ab"), b"");
        assert_eq!(encoder.push(b"cde"), b"abc");
        assert_eq!(encoder.push(b"f"), b"def");
        assert_eq!(encoder.push(b"g"), b"");
        assert_eq!(encoder.finish(), b"g");
    }
}
