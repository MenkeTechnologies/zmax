use std::io;

use ropey::iter::Chunks;
use ropey::RopeSlice;

pub struct RopeReader<'a> {
    current_chunk: &'a [u8],
    chunks: Chunks<'a>,
}

impl<'a> RopeReader<'a> {
    pub fn new(rope: RopeSlice<'a>) -> RopeReader<'a> {
        RopeReader {
            current_chunk: &[],
            chunks: rope.chunks(),
        }
    }
}

impl io::Read for RopeReader<'_> {
    fn read(&mut self, mut buf: &mut [u8]) -> io::Result<usize> {
        let buf_len = buf.len();
        loop {
            let read_bytes = self.current_chunk.read(buf)?;
            buf = &mut buf[read_bytes..];
            if buf.is_empty() {
                return Ok(buf_len);
            }

            if let Some(next_chunk) = self.chunks.next() {
                self.current_chunk = next_chunk.as_bytes();
            } else {
                return Ok(buf_len - buf.len());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rope;
    use std::io::Read;

    /// Long enough that ropey stores it in several chunks, which is the only way
    /// to exercise the loop that advances to the next one. The multi-byte
    /// characters make a chunk boundary land mid-codepoint.
    fn multi_chunk_rope() -> Rope {
        let mut text = String::new();
        for line in 0..400 {
            text.push_str(&format!("line {line}: naïve café 日本語 — ok\n"));
        }
        let rope = Rope::from(text.as_str());
        assert!(
            rope.slice(..).chunks().count() > 1,
            "the fixture must span chunks, got {}",
            rope.slice(..).chunks().count()
        );
        rope
    }

    /// Reading to the end reproduces the rope exactly, whatever the buffer size.
    /// A one-byte buffer is the worst case: every chunk boundary and every
    /// multi-byte character is crossed by a separate `read` call.
    #[test]
    fn reading_reproduces_the_rope_at_any_buffer_size() {
        let rope = multi_chunk_rope();
        let expected = rope.to_string();

        for buf_size in [1, 7, 4096, expected.len() + 10] {
            let mut reader = RopeReader::new(rope.slice(..));
            let mut out = Vec::new();
            let mut buf = vec![0; buf_size];
            loop {
                let read = reader.read(&mut buf).unwrap();
                if read == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..read]);
            }

            assert_eq!(
                String::from_utf8(out).unwrap(),
                expected,
                "buffer size {buf_size}"
            );
        }
    }

    /// A reader over a slice yields the slice, not the whole rope -- callers hand
    /// it `doc.slice(range)` to feed a region to an external program.
    #[test]
    fn a_slice_reader_stops_at_the_slice() {
        let rope = Rope::from("hello world");
        let mut out = String::new();

        RopeReader::new(rope.slice(0..5))
            .read_to_string(&mut out)
            .unwrap();

        assert_eq!(out, "hello");
    }

    /// Once drained the reader keeps reporting end of stream rather than
    /// restarting or erroring, so `read_to_end` terminates.
    #[test]
    fn a_drained_reader_stays_at_end_of_stream() {
        let rope = Rope::from("abc");
        let mut reader = RopeReader::new(rope.slice(..));
        let mut buf = [0; 8];

        assert_eq!(reader.read(&mut buf).unwrap(), 3);
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
        assert_eq!(reader.read(&mut buf).unwrap(), 0, "still at the end");
    }

    /// An empty rope reads as an empty stream, and a zero-length buffer consumes
    /// nothing -- neither is an error, and neither loops forever.
    #[test]
    fn empty_ropes_and_empty_buffers_read_nothing() {
        let empty = Rope::new();
        let mut out = Vec::new();
        RopeReader::new(empty.slice(..)).read_to_end(&mut out).unwrap();
        assert!(out.is_empty());

        let rope = Rope::from("abc");
        let mut reader = RopeReader::new(rope.slice(..));
        assert_eq!(reader.read(&mut []).unwrap(), 0, "nothing asked for");
        // ...and the content is still there to read afterwards.
        let mut rest = String::new();
        reader.read_to_string(&mut rest).unwrap();
        assert_eq!(rest, "abc");
    }
}
