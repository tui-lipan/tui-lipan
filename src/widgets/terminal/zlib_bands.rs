//! Zlib streams for large relayed frames, deflated in parallel bands.
//!
//! A full-pane frame is several MiB, and deflating it on one core takes longer than a frame
//! interval. Each band here is deflated on its own and ended with a sync flush, which leaves the
//! stream byte-aligned at a block boundary, so the bands concatenate into one ordinary zlib stream
//! that any inflater reads. A band starts without the previous band's history, which costs a
//! little ratio and nothing else.

use std::num::NonZeroUsize;

use flate2::{Compress, Compression, FlushCompress, Status};

/// Below this, one band is cheap enough that splitting would only add thread start-up.
const MIN_BAND: usize = 512 * 1024;
/// Upper bound on bands, and so threads, for one frame. More stops paying off well before this.
const MAX_BANDS: usize = 8;
/// The two-byte zlib header for deflate with a 32 KiB window at the fastest level.
const HEADER: [u8; 2] = [0x78, 0x01];

/// Compress `data` as one zlib stream at the fastest level.
pub(super) fn compress(data: &[u8]) -> Option<Vec<u8>> {
    let threads = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    let bands = (data.len() / MIN_BAND).clamp(1, threads.min(MAX_BANDS));
    if bands == 1 {
        return compress_serially(data);
    }
    let band = data.len().div_ceil(bands);
    let (deflated, checksum) = std::thread::scope(|scope| {
        let chunks: Vec<_> = data.chunks(band).collect();
        let last = chunks.len() - 1;
        let workers: Vec<_> = chunks
            .into_iter()
            .enumerate()
            .map(|(index, chunk)| scope.spawn(move || deflate(chunk, index == last)))
            .collect();
        let mut checksum = simd_adler32::Adler32::new();
        checksum.write(data);
        let deflated: Option<Vec<_>> = workers
            .into_iter()
            .map(|worker| worker.join().ok().flatten())
            .collect();
        (deflated, checksum.finish())
    });
    let deflated = deflated?;
    let mut out = Vec::with_capacity(deflated.iter().map(Vec::len).sum::<usize>() + 6);
    out.extend_from_slice(&HEADER);
    for band in deflated {
        out.extend_from_slice(&band);
    }
    out.extend_from_slice(&checksum.to_be_bytes());
    Some(out)
}

fn compress_serially(data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write as _;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(data).ok()?;
    encoder.finish().ok()
}

/// Raw deflate of one band. Every band but the last ends with a sync flush; the last finishes
/// the stream.
fn deflate(band: &[u8], last: bool) -> Option<Vec<u8>> {
    let mut compress = Compress::new(Compression::fast(), false);
    let flush = if last {
        FlushCompress::Finish
    } else {
        FlushCompress::Sync
    };
    let mut out = Vec::with_capacity(band.len() / 2 + 64);
    loop {
        if out.len() == out.capacity() {
            out.reserve(band.len() / 4 + 64);
        }
        let consumed = usize::try_from(compress.total_in()).ok()?;
        let status = compress
            .compress_vec(&band[consumed..], &mut out, flush)
            .ok()?;
        let drained = usize::try_from(compress.total_in()).ok()? == band.len();
        // A flush is complete once all input is in and the output did not fill up.
        let flushed = if last {
            status == Status::StreamEnd
        } else {
            drained && out.len() < out.capacity()
        };
        if flushed {
            return Some(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    fn inflate(stream: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(stream)
            .read_to_end(&mut out)
            .unwrap();
        out
    }

    #[test]
    fn banded_streams_inflate_to_the_original_bytes() {
        // Pseudo-random pixels with runs, so bands hold both literals and matches.
        let mut state = 0x2545_f491_u32;
        let data: Vec<u8> = (0..MIN_BAND * 5 + 12_345)
            .map(|index| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                if index % 7 < 3 { 0 } else { state as u8 }
            })
            .collect();
        for length in [
            0,
            1,
            MIN_BAND - 1,
            MIN_BAND * 2,
            MIN_BAND * 2 + 1,
            data.len(),
        ] {
            let stream = compress(&data[..length]).unwrap();
            assert_eq!(inflate(&stream), &data[..length], "length {length}");
        }
    }

    #[test]
    fn a_banded_stream_carries_the_whole_input_checksum() {
        let data = vec![7u8; MIN_BAND * 4];
        let stream = compress(&data).unwrap();
        let mut checksum = simd_adler32::Adler32::new();
        checksum.write(&data);
        assert_eq!(stream[stream.len() - 4..], checksum.finish().to_be_bytes());
        // A corrupted trailer must be rejected, which proves the decoder checks it.
        let mut corrupt = stream.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        let mut out = Vec::new();
        assert!(
            flate2::read::ZlibDecoder::new(&corrupt[..])
                .read_to_end(&mut out)
                .is_err()
        );
    }
}
