//! Reading messages out of escape codes, and the data streams that span several messages.
//!
//! Both directions of the protocol carry two shapes of payload. Text (MIME lists, machine ids,
//! errors) is small and read whole. Data (dropped contents, files, drag images) is base64, can be
//! arbitrarily large, and arrives as a run of messages ended by one with an empty payload.
//! [`MessageReader`] turns escape codes into those two shapes, without holding data in memory.

use super::base64::{Decoder, InvalidBase64};
use super::wire::{Frame, FrameReader, Metadata};

/// The most text one message may carry before it is refused.
pub const MAX_TEXT: usize = 1 << 20;

/// One message, or one piece of a data message, read from escape codes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Read {
    /// A whole text message.
    Text { meta: Metadata, text: Vec<u8> },
    /// Decoded bytes of a data message. More may follow in the same message.
    Data { meta: Metadata, bytes: Vec<u8> },
    /// A data message ended. `empty` when it carried nothing: the end of its stream.
    DataEnd { meta: Metadata, empty: bool },
    /// A message could not be read; carries the metadata it would have had.
    Invalid { meta: Metadata },
}

/// Turns escape codes into messages, given which message types carry data.
#[derive(Clone, Debug)]
pub(crate) struct MessageReader {
    frames: FrameReader,
    is_data: fn(&Metadata) -> bool,
    text: Vec<u8>,
    text_overflow: bool,
    decoder: Decoder,
    data_seen: bool,
    data_failed: bool,
}

impl MessageReader {
    pub(crate) fn new(is_data: fn(&Metadata) -> bool) -> Self {
        Self {
            frames: FrameReader::default(),
            is_data,
            text: Vec::new(),
            text_overflow: false,
            decoder: Decoder::default(),
            data_seen: false,
            data_failed: false,
        }
    }

    pub(crate) fn push(&mut self, frame: Frame) -> Vec<Read> {
        let fragment = self.frames.push(frame);
        let meta = fragment.meta;
        if fragment.meta.kind() == b'q' {
            return vec![Read::Text {
                meta,
                text: fragment.payload,
            }];
        }
        if fragment.first {
            self.text.clear();
            self.text_overflow = false;
            self.decoder = Decoder::default();
            self.data_seen = false;
            self.data_failed = false;
        }
        let mut out = Vec::new();
        if (self.is_data)(&meta) {
            if !fragment.payload.is_empty() {
                self.data_seen = true;
            }
            if !self.data_failed {
                let mut bytes = Vec::new();
                let result = self.decode(&fragment.payload, fragment.last, &mut bytes);
                if result.is_err() {
                    self.data_failed = true;
                    out.push(Read::Invalid { meta });
                } else if !bytes.is_empty() {
                    out.push(Read::Data { meta, bytes });
                }
            }
            if fragment.last && !self.data_failed {
                out.push(Read::DataEnd {
                    meta,
                    empty: !self.data_seen,
                });
            }
        } else {
            if self.text.len() + fragment.payload.len() > MAX_TEXT {
                self.text_overflow = true;
            } else {
                self.text.extend_from_slice(&fragment.payload);
            }
            if fragment.last {
                let text = std::mem::take(&mut self.text);
                out.push(if self.text_overflow {
                    Read::Invalid { meta }
                } else {
                    Read::Text { meta, text }
                });
            }
        }
        out
    }

    fn decode(
        &mut self,
        payload: &[u8],
        last: bool,
        out: &mut Vec<u8>,
    ) -> Result<(), InvalidBase64> {
        self.decoder.push(payload, out)?;
        if last {
            self.decoder.finish(out)?;
        }
        Ok(())
    }

    /// Forget a half-read message.
    pub(crate) fn reset(&mut self) {
        self.frames.reset();
        self.text.clear();
        self.decoder = Decoder::default();
    }
}

/// Encodes one data stream as messages, keeping every message but the last a whole number of
/// base64 groups.
///
/// The reference terminal decodes a stream as one continuous encoding, while other readers decode
/// each message on its own; whole groups read the same either way.
#[derive(Clone, Debug)]
pub struct DataStreamWriter {
    meta: Metadata,
    carry: super::base64::StreamEncoder,
}

impl DataStreamWriter {
    /// A writer whose messages all carry `meta`.
    pub fn new(meta: Metadata) -> Self {
        Self {
            meta,
            carry: super::base64::StreamEncoder::default(),
        }
    }

    /// Append the messages that carry `bytes`, holding back a partial group.
    pub fn write(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
        let whole = self.carry.push(bytes);
        if !whole.is_empty() {
            super::wire::write_message(out, self.meta, &whole, true);
        }
    }

    /// Append the rest of the data and the empty message that ends the stream.
    pub fn finish(mut self, out: &mut Vec<u8>) {
        let rest = self.carry.finish();
        if !rest.is_empty() {
            super::wire::write_message(out, self.meta, &rest, true);
        }
        super::wire::write_message(out, self.meta, &[], true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dnd::wire::{OscScanner, write_message};

    fn read_all(reader: &mut MessageReader, bytes: &[u8]) -> Vec<Read> {
        OscScanner::default()
            .scan(bytes)
            .into_iter()
            .flat_map(|(_, body)| reader.push(Frame::parse(&body).unwrap()))
            .collect()
    }

    #[test]
    fn a_data_stream_reads_back_whatever_the_write_sizes() {
        let data: Vec<u8> = (0..20_000u32).map(|n| (n * 7 % 256) as u8).collect();
        for step in [1, 2, 5, 3072, 7000] {
            let mut out = Vec::new();
            let mut writer = DataStreamWriter::new(Metadata {
                x: Some(1),
                ..Metadata::of(b'r')
            });
            for chunk in data.chunks(step) {
                writer.write(chunk, &mut out);
            }
            writer.finish(&mut out);
            let mut reader = MessageReader::new(|m| m.kind() == b'r');
            let mut back = Vec::new();
            let mut ended = 0;
            for read in read_all(&mut reader, &out) {
                match read {
                    Read::Data { bytes, .. } => back.extend(bytes),
                    Read::DataEnd { empty: true, .. } => ended += 1,
                    Read::DataEnd { empty: false, .. } => {}
                    other => panic!("unexpected {other:?}"),
                }
            }
            assert_eq!(ended, 1, "step {step}");
            assert_eq!(back, data, "step {step}");
        }
    }

    #[test]
    fn text_is_read_whole_and_bounded() {
        let mut out = Vec::new();
        let list = "text/plain ".repeat(1000);
        write_message(&mut out, Metadata::of(b'a'), list.as_bytes(), false);
        let mut reader = MessageReader::new(|_| false);
        let reads = read_all(&mut reader, &out);
        assert_eq!(
            reads,
            [Read::Text {
                meta: Metadata::of(b'a'),
                text: list.into_bytes(),
            }]
        );
        let mut huge = Vec::new();
        write_message(
            &mut huge,
            Metadata::of(b'a'),
            &vec![b'a'; MAX_TEXT + 1],
            false,
        );
        assert!(matches!(
            read_all(&mut reader, &huge).as_slice(),
            [Read::Invalid { .. }]
        ));
    }

    #[test]
    fn bad_base64_fails_the_message_once() {
        let mut reader = MessageReader::new(|_| true);
        let reads = read_all(
            &mut reader,
            b"\x1b]72;t=r:m=1;QU!D\x1b\\\x1b]72;t=r;QUJD\x1b\\",
        );
        assert!(matches!(reads.as_slice(), [Read::Invalid { .. }]));
        let next = read_all(&mut reader, b"\x1b]72;t=r;QUJD\x1b\\");
        assert!(matches!(
            next.as_slice(),
            [Read::Data { .. }, Read::DataEnd { .. }]
        ));
    }
}
