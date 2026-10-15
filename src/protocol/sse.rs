//! Pull-based SSE framing: consume at most one frame per call, even when the
//! network hands us a huge chunk. The caller retains the unconsumed bytes.
use super::Error;

pub const FRAME_LIMIT: usize = 1024 * 1024;

#[derive(Default)]
pub struct Parser {
    frame: Vec<u8>,
    line_nonempty: bool,
    skip_lf: bool,
    started: bool,
    bom_bytes: usize,
}

pub struct Frame(Vec<u8>);
#[derive(Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: String,
    pub data: String,
}

impl Parser {
    /// Returns consumed bytes and at most one frame. Never queues a second
    /// frame behind a slow consumer and never copies a whole transport chunk.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<(usize, Option<Frame>), Error> {
        for (i, &b) in chunk.iter().enumerate() {
            if !self.started {
                if self.bom_bytes > 0 || b == 0xef {
                    let expected = [0xef, 0xbb, 0xbf][self.bom_bytes];
                    if b != expected {
                        return Err(Error::upstream("invalid_sse_utf8"));
                    }
                    self.bom_bytes += 1;
                    if self.bom_bytes == 3 {
                        self.started = true;
                        self.bom_bytes = 0;
                    }
                    continue;
                }
                self.started = true;
            }
            if self.skip_lf {
                self.skip_lf = false;
                if b == b'\n' {
                    continue;
                }
            }
            if self.frame.len() >= FRAME_LIMIT {
                return Err(Error::upstream("sse_frame_too_large"));
            }
            self.frame.push(b);
            if b == b'\r' || b == b'\n' {
                self.skip_lf = b == b'\r';
                if !self.line_nonempty {
                    return Ok((i + 1, Some(Frame(std::mem::take(&mut self.frame)))));
                }
                self.line_nonempty = false;
            } else {
                self.line_nonempty = true;
            }
        }
        Ok((chunk.len(), None))
    }

    pub fn finish(&self) -> Result<(), Error> {
        if !self.frame.is_empty() || self.bom_bytes != 0 {
            return Err(Error::upstream("incomplete_sse_frame"));
        }
        Ok(())
    }

    #[cfg(test)]
    fn resident_bytes(&self) -> usize {
        self.frame.capacity()
    }
}

impl Frame {
    /// Remove framing in place. The data String owns the existing allocation;
    /// a second 1MiB JSON string is not built alongside a retained raw frame.
    pub fn into_event(mut self) -> Result<Option<Event>, Error> {
        std::str::from_utf8(&self.0).map_err(|_| Error::upstream("invalid_sse_utf8"))?;
        let mut kind = String::from("message");
        let mut read = 0;
        let mut write = 0;
        let mut data_seen = false;
        while read < self.0.len() {
            let start = read;
            while read < self.0.len() && !matches!(self.0[read], b'\r' | b'\n') {
                read += 1;
            }
            let end = read;
            while read < self.0.len() && matches!(self.0[read], b'\r' | b'\n') {
                read += 1;
            }
            if start == end || self.0[start] == b':' {
                continue;
            }
            let colon = self.0[start..end]
                .iter()
                .position(|b| *b == b':')
                .map(|v| start + v);
            let (key_end, mut value) = colon.map_or((end, end), |v| (v, v + 1));
            if value < end && self.0[value] == b' ' {
                value += 1;
            }
            match &self.0[start..key_end] {
                b"event" => {
                    if end - value > 128 {
                        return Err(Error::upstream("invalid_sse_event"));
                    }
                    kind = std::str::from_utf8(&self.0[value..end])
                        .expect("validated UTF-8")
                        .into();
                    if kind.is_empty() {
                        kind = "message".into();
                    }
                }
                b"data" => {
                    data_seen = true;
                    let length = end - value;
                    self.0.copy_within(value..end, write);
                    write += length;
                    // Every complete data line had a newline in the raw frame,
                    // leaving space for its SSE concatenation separator.
                    self.0[write] = b'\n';
                    write += 1;
                }
                _ => (), // SSE id/retry/extensions do not carry response data.
            }
        }
        if !data_seen {
            return Ok(None);
        }
        self.0.truncate(write.saturating_sub(1));
        let data = String::from_utf8(self.0).map_err(|_| Error::upstream("invalid_sse_utf8"))?;
        Ok(Some(Event { kind, data }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn collect(chunks: &[&[u8]]) -> Result<Vec<Event>, Error> {
        let mut parser = Parser::default();
        let mut events = vec![];
        for chunk in chunks {
            let mut rest = *chunk;
            while !rest.is_empty() {
                let (used, frame) = parser.feed(rest)?;
                rest = &rest[used..];
                if let Some(frame) = frame
                    && let Some(event) = frame.into_event()?
                {
                    events.push(event);
                }
            }
        }
        parser.finish()?;
        Ok(events)
    }
    #[test]
    fn all_chunk_boundaries_preserve_utf8_bom_crlf_and_multiline_data() {
        let bytes = "\u{feff}: ping\r\nevent: content_block_delta\r\ndata: {\"text\":\"中文\",\r\ndata: \"x\":1}\r\n\r\ndata: next\n\n".as_bytes();
        let expected = vec![
            Event {
                kind: "content_block_delta".into(),
                data: "{\"text\":\"中文\",\n\"x\":1}".into(),
            },
            Event {
                kind: "message".into(),
                data: "next".into(),
            },
        ];
        for i in 0..=bytes.len() {
            assert_eq!(
                collect(&[&bytes[..i], &bytes[i..]]).unwrap(),
                expected,
                "split {i}"
            );
        }
        let chunks = bytes.chunks(1).collect::<Vec<_>>();
        assert_eq!(collect(&chunks).unwrap(), expected);
    }
    #[test]
    fn huge_transport_chunk_yields_only_one_frame_and_partial_eof_is_error() {
        let bytes = b"data: one\n\ndata: two\n\n";
        let mut parser = Parser::default();
        let (consumed, frame) = parser.feed(bytes).unwrap();
        assert_eq!(consumed, 11);
        assert_eq!(frame.unwrap().into_event().unwrap().unwrap().data, "one");
        assert_eq!(parser.resident_bytes(), 0);
        assert_eq!(
            collect(&[b"data: unfinished"]).unwrap_err().code,
            "incomplete_sse_frame"
        );
        assert_eq!(
            collect(&[b"data: \xff\n\n"]).unwrap_err().code,
            "invalid_sse_utf8"
        );
        assert!(
            collect(&[b": ping\n\ndata:\n\n"]).unwrap()[0]
                .data
                .is_empty()
        );
        assert_eq!(collect(&[b"data: one\r\rdata: two\r\r"]).unwrap().len(), 2);
    }
    #[test]
    fn oversized_frames_fail_before_retaining_unbounded_chunks() {
        let mut parser = Parser::default();
        let bytes = vec![b'x'; FRAME_LIMIT + 65536];
        assert_eq!(
            parser.feed(&bytes).err().unwrap().code,
            "sse_frame_too_large"
        );
        assert!(parser.resident_bytes() <= FRAME_LIMIT);
    }
}
