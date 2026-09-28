// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use std::io::{self, BufRead, ErrorKind};

/// Maximum number of raw bytes in a frame's headers, including line endings
/// and the blank line separating the headers from the payload.
pub const MAX_HEADER_BYTES: usize = 8 * 1024;

/// Maximum accepted payload size. The limit is checked before allocating.
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

/// Reads one Content-Length-framed message without decoding its payload.
///
/// Both CRLF and LF line endings are accepted. EOF is clean only before any
/// bytes of the next frame have been read.
pub fn read_frame<R: BufRead>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut header_bytes = 0;
    let mut line = Vec::new();
    let mut content_length = None;

    loop {
        line.clear();
        let found_line_ending = read_bounded_line(
            reader,
            &mut line,
            MAX_HEADER_BYTES.saturating_sub(header_bytes),
        )?;

        if line.is_empty() && !found_line_ending {
            return if header_bytes == 0 {
                Ok(None)
            } else {
                Err(unexpected_eof("EOF while reading frame headers"))
            };
        }
        if !found_line_ending {
            return Err(unexpected_eof("EOF while reading frame headers"));
        }

        header_bytes = header_bytes
            .checked_add(line.len())
            .ok_or_else(|| invalid_data("frame headers exceed the byte limit"))?;

        let header = strip_line_ending(&line)?;
        if header.is_empty() {
            break;
        }

        let (name, value) = parse_header(header)?;
        if name.eq_ignore_ascii_case(b"Content-Length") {
            if content_length.is_some() {
                return Err(invalid_data("duplicate Content-Length header"));
            }
            content_length = Some(parse_content_length(trim_ascii_whitespace(value))?);
        }
    }

    let content_length =
        content_length.ok_or_else(|| invalid_data("missing Content-Length header"))?;
    if content_length > MAX_PAYLOAD_BYTES {
        return Err(invalid_data("frame payload exceeds the byte limit"));
    }

    let mut payload = Vec::new();
    payload
        .try_reserve_exact(content_length)
        .map_err(|error| io::Error::other(format!("failed to allocate frame payload: {error}")))?;
    payload.resize(content_length, 0);
    reader
        .read_exact(&mut payload)
        .map_err(|error| match error.kind() {
            ErrorKind::UnexpectedEof => unexpected_eof("EOF while reading frame payload"),
            _ => error,
        })?;
    Ok(Some(payload))
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    line: &mut Vec<u8>,
    byte_limit: usize,
) -> io::Result<bool> {
    loop {
        let buffer = loop {
            match reader.fill_buf() {
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                result => break result?,
            }
        };
        if buffer.is_empty() {
            return Ok(false);
        }

        let bytes_to_take = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |index| index + 1);
        let remaining = byte_limit.saturating_sub(line.len());
        if bytes_to_take > remaining {
            return Err(invalid_data("frame headers exceed the byte limit"));
        }

        line.extend_from_slice(&buffer[..bytes_to_take]);
        let found_line_ending = buffer[bytes_to_take - 1] == b'\n';
        reader.consume(bytes_to_take);
        if found_line_ending {
            return Ok(true);
        }
    }
}

fn strip_line_ending(line: &[u8]) -> io::Result<&[u8]> {
    let without_lf = line
        .strip_suffix(b"\n")
        .ok_or_else(|| invalid_data("header line is not terminated"))?;
    Ok(without_lf.strip_suffix(b"\r").unwrap_or(without_lf))
}

fn parse_header(header: &[u8]) -> io::Result<(&[u8], &[u8])> {
    let colon = header
        .iter()
        .position(|byte| *byte == b':')
        .ok_or_else(|| invalid_data("malformed header"))?;
    let name = &header[..colon];
    let value = &header[colon + 1..];

    if name.is_empty() || !name.iter().all(|byte| is_header_name_byte(*byte)) {
        return Err(invalid_data("malformed header name"));
    }
    if !value
        .iter()
        .all(|byte| *byte == b'\t' || (b' '..=b'~').contains(byte))
    {
        return Err(invalid_data("malformed header value"));
    }

    Ok((name, value))
}

fn is_header_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn trim_ascii_whitespace(mut value: &[u8]) -> &[u8] {
    while matches!(value.first(), Some(b' ' | b'\t')) {
        value = &value[1..];
    }
    while matches!(value.last(), Some(b' ' | b'\t')) {
        value = &value[..value.len() - 1];
    }
    value
}

fn parse_content_length(value: &[u8]) -> io::Result<usize> {
    if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
        return Err(invalid_data(
            "Content-Length must be a non-negative decimal integer",
        ));
    }

    value.iter().try_fold(0usize, |length, byte| {
        length
            .checked_mul(10)
            .and_then(|length| length.checked_add(usize::from(*byte - b'0')))
            .ok_or_else(|| invalid_data("Content-Length overflows usize"))
    })
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}

fn unexpected_eof(message: &'static str) -> io::Error {
    io::Error::new(ErrorKind::UnexpectedEof, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp;
    use std::io::{BufReader, Cursor, Read};

    fn frame_with_line_ending(payload: &[u8], line_ending: &[u8]) -> Vec<u8> {
        let mut frame = format!("Content-Length: {}", payload.len()).into_bytes();
        frame.extend_from_slice(line_ending);
        frame.extend_from_slice(line_ending);
        frame.extend_from_slice(payload);
        frame
    }

    fn error_kind(input: &[u8]) -> ErrorKind {
        read_frame(&mut Cursor::new(input))
            .expect_err("frame should be rejected")
            .kind()
    }

    #[test]
    fn accepts_multiple_headers_in_any_order_and_case() {
        let input = b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n\
                      X-Extension: value\r\n\
                      cOnTeNt-LeNgTh: 7\r\n\r\n\
                      {\"x\":1}";
        assert_eq!(
            read_frame(&mut Cursor::new(input)).unwrap(),
            Some(br#"{"x":1}"#.to_vec())
        );

        let input = b"X-Before: yes\nContent-Length: 2\nX-After: yes\n\n{}";
        assert_eq!(
            read_frame(&mut Cursor::new(input)).unwrap(),
            Some(b"{}".to_vec())
        );
    }

    #[test]
    fn reads_consecutive_crlf_and_lf_frames() {
        let mut input = frame_with_line_ending(b"one", b"\r\n");
        input.extend(frame_with_line_ending(b"two", b"\n"));
        let mut reader = Cursor::new(input);

        assert_eq!(read_frame(&mut reader).unwrap(), Some(b"one".to_vec()));
        assert_eq!(read_frame(&mut reader).unwrap(), Some(b"two".to_vec()));
        assert_eq!(read_frame(&mut reader).unwrap(), None);
    }

    #[test]
    fn preserves_unicode_payload_bytes() {
        let payload = "snowman: \u{2603}; crab: \u{1f980}".as_bytes();
        let input = frame_with_line_ending(payload, b"\r\n");
        assert_eq!(
            read_frame(&mut Cursor::new(input)).unwrap(),
            Some(payload.to_vec())
        );
    }

    #[test]
    fn supports_zero_length_payload() {
        let mut reader = Cursor::new(b"Content-Length: 0\r\n\r\nnext");
        assert_eq!(read_frame(&mut reader).unwrap(), Some(Vec::new()));
        assert_eq!(reader.position(), 21);
    }

    #[test]
    fn distinguishes_clean_and_truncated_eof() {
        assert_eq!(read_frame(&mut Cursor::new(b"")).unwrap(), None);
        assert_eq!(error_kind(b"Content-Length: 1"), ErrorKind::UnexpectedEof);
        assert_eq!(error_kind(b"Content-Length: 1\n"), ErrorKind::UnexpectedEof);
        assert_eq!(
            error_kind(b"Content-Length: 3\n\nab"),
            ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn rejects_invalid_content_lengths() {
        for input in [
            b"Content-Length:\n\n".as_slice(),
            b"Content-Length: -1\n\n",
            b"Content-Length: +1\n\n",
            b"Content-Length: 1.0\n\n",
            b"Content-Length: 1 0\n\n",
            b"Content-Length: \xff\n\n",
        ] {
            assert_eq!(error_kind(input), ErrorKind::InvalidData, "{input:?}");
        }

        let overflow = format!("Content-Length: {}0\n\n", usize::MAX);
        assert_eq!(error_kind(overflow.as_bytes()), ErrorKind::InvalidData);
        let over_limit = format!("Content-Length: {}\n\n", MAX_PAYLOAD_BYTES + 1);
        assert_eq!(error_kind(over_limit.as_bytes()), ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_duplicate_and_missing_content_length() {
        assert_eq!(
            error_kind(b"Content-Length: 0\ncontent-length: 0\n\n"),
            ErrorKind::InvalidData
        );
        assert_eq!(
            error_kind(b"Content-Type: application/json\n\n"),
            ErrorKind::InvalidData
        );
    }

    #[test]
    fn rejects_malformed_headers() {
        for input in [
            b"Content Length: 0\n\n".as_slice(),
            b"Content-Length 0\n\n",
            b": value\nContent-Length: 0\n\n",
            b"X: value\rcontinued\nContent-Length: 0\n\n",
            b"X: \x7f\nContent-Length: 0\n\n",
        ] {
            assert_eq!(error_kind(input), ErrorKind::InvalidData, "{input:?}");
        }
    }

    fn header_of_size(size: usize) -> Vec<u8> {
        let fixed = b"X: \nContent-Length: 0\n\n";
        assert!(size >= fixed.len());
        let mut header = b"X: ".to_vec();
        header.resize(size - (fixed.len() - 3), b'a');
        header.extend_from_slice(b"\nContent-Length: 0\n\n");
        assert_eq!(header.len(), size);
        header
    }

    #[test]
    fn enforces_exact_header_byte_limit() {
        let at_limit = header_of_size(MAX_HEADER_BYTES);
        assert_eq!(
            read_frame(&mut Cursor::new(at_limit)).unwrap(),
            Some(Vec::new())
        );

        let over_limit = header_of_size(MAX_HEADER_BYTES + 1);
        assert_eq!(error_kind(&over_limit), ErrorKind::InvalidData);
    }

    #[test]
    fn accepts_exact_payload_limit_without_consuming_the_next_frame() {
        let header = format!("Content-Length: {MAX_PAYLOAD_BYTES}\r\n\r\n");
        let input = Cursor::new(header.into_bytes())
            .chain(io::repeat(b'x').take(MAX_PAYLOAD_BYTES as u64))
            .chain(Cursor::new(b"Content-Length: 0\n\n"));
        let mut reader = BufReader::new(input);
        let payload = read_frame(&mut reader).unwrap().unwrap();
        assert_eq!(payload.len(), MAX_PAYLOAD_BYTES);
        assert!(payload.iter().all(|byte| *byte == b'x'));
        assert_eq!(read_frame(&mut reader).unwrap(), Some(Vec::new()));
        assert_eq!(read_frame(&mut reader).unwrap(), None);
    }

    #[test]
    fn bounds_unterminated_header_lines() {
        assert_eq!(
            error_kind(&vec![b'a'; MAX_HEADER_BYTES]),
            ErrorKind::UnexpectedEof
        );
        assert_eq!(
            error_kind(&vec![b'a'; MAX_HEADER_BYTES + 1]),
            ErrorKind::InvalidData
        );
    }

    struct FragmentedReader {
        data: Vec<u8>,
        position: usize,
        chunks: Vec<usize>,
        next_chunk: usize,
    }

    impl FragmentedReader {
        fn new(data: Vec<u8>, chunks: Vec<usize>) -> Self {
            Self {
                data,
                position: 0,
                chunks,
                next_chunk: 0,
            }
        }
    }

    impl Read for FragmentedReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.position == self.data.len() {
                return Ok(0);
            }
            let chunk = self.chunks[self.next_chunk % self.chunks.len()];
            self.next_chunk += 1;
            let length = cmp::min(
                chunk,
                cmp::min(buffer.len(), self.data.len() - self.position),
            );
            buffer[..length].copy_from_slice(&self.data[self.position..self.position + length]);
            self.position += length;
            Ok(length)
        }
    }

    #[test]
    fn handles_deterministic_fragmentation_patterns() {
        let payloads = [b"".as_slice(), b"x", br#"{"unicode":"\u2603","value":42}"#];
        let patterns = [
            vec![1],
            vec![2, 1, 3],
            vec![7, 4, 1, 9, 2],
            (1..=17).collect(),
        ];

        for payload in payloads {
            let frame = frame_with_line_ending(payload, b"\r\n");
            for chunks in &patterns {
                let fragmented = FragmentedReader::new(frame.clone(), chunks.clone());
                let mut reader = BufReader::with_capacity(5, fragmented);
                assert_eq!(
                    read_frame(&mut reader).unwrap(),
                    Some(payload.to_vec()),
                    "payload {payload:?}, chunks {chunks:?}"
                );
                assert_eq!(read_frame(&mut reader).unwrap(), None);
            }
        }

        for seed in 0..32u32 {
            let mut state = seed.wrapping_add(1);
            let payload_length = (seed as usize * 37) % 257;
            let payload: Vec<u8> = (0..payload_length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    state.to_le_bytes()[2]
                })
                .collect();
            let chunks: Vec<usize> = (0..23)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    usize::from(state.to_le_bytes()[1] % 19) + 1
                })
                .collect();
            let line_ending: &[u8] = if seed % 2 == 0 { b"\r\n" } else { b"\n" };
            let frame = frame_with_line_ending(&payload, line_ending);
            let fragmented = FragmentedReader::new(frame, chunks);
            let mut reader = BufReader::with_capacity((seed as usize % 11) + 1, fragmented);

            assert_eq!(read_frame(&mut reader).unwrap(), Some(payload));
            assert_eq!(read_frame(&mut reader).unwrap(), None);
        }
    }

    struct InterruptingReader {
        inner: Cursor<Vec<u8>>,
        interrupt_next: bool,
    }

    impl Read for InterruptingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.interrupt_next {
                self.interrupt_next = false;
                return Err(io::Error::new(ErrorKind::Interrupted, "interrupted"));
            }
            self.interrupt_next = true;
            self.inner.read(buffer)
        }
    }

    #[test]
    fn retries_interrupted_header_and_payload_reads() {
        let input = frame_with_line_ending(b"payload", b"\r\n");
        let interrupting = InterruptingReader {
            inner: Cursor::new(input),
            interrupt_next: true,
        };
        let mut reader = BufReader::with_capacity(3, interrupting);
        assert_eq!(read_frame(&mut reader).unwrap(), Some(b"payload".to_vec()));
    }

    struct ErrorAfterReader {
        inner: Cursor<Vec<u8>>,
        bytes_before_error: usize,
    }

    impl Read for ErrorAfterReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.bytes_before_error == 0 {
                return Err(io::Error::other("reader failed"));
            }
            let limit = cmp::min(buffer.len(), self.bytes_before_error);
            let read = self.inner.read(&mut buffer[..limit])?;
            self.bytes_before_error -= read;
            Ok(read)
        }
    }

    #[test]
    fn propagates_actual_reader_errors_in_headers_and_payload() {
        for bytes_before_error in [5, b"Content-Length: 4\r\n\r\n".len() + 2] {
            let input = frame_with_line_ending(b"data", b"\r\n");
            let failing = ErrorAfterReader {
                inner: Cursor::new(input),
                bytes_before_error,
            };
            let mut reader = BufReader::with_capacity(3, failing);
            assert_eq!(
                read_frame(&mut reader).unwrap_err().kind(),
                ErrorKind::Other
            );
        }
    }
}
