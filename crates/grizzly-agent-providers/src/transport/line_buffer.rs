//! Splits a byte stream into whole lines across arbitrary chunk boundaries.
//!
//! The transport hands over bytes in whatever pieces the connection happens
//! to deliver — a chunk boundary can fall anywhere, including mid-line and
//! mid-UTF-8 sequence. Bytes are held until a newline arrives rather than
//! decoded as they arrive, so a chunk boundary never has to line up with a
//! character boundary: only a whole, newline-terminated line is ever decoded.

/// Buffers raw bytes until they form whole lines.
#[derive(Debug, Default)]
pub(crate) struct LineBuffer {
    pending: Vec<u8>,
}

impl LineBuffer {
    /// Append `bytes`, returning every line they complete. `\r\n` and `\n`
    /// line endings are both accepted; the terminator is stripped.
    ///
    /// A line that is not valid UTF-8 is decoded lossily (invalid sequences
    /// become U+FFFD) rather than rejected here: on every provider this
    /// crate speaks, a line carries JSON, so bytes corrupted in transit fail
    /// to parse as JSON downstream and surface as that provider's normal
    /// malformed-chunk failure instead of a second error path for the same
    /// underlying problem.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.pending.extend_from_slice(bytes);
        let mut lines = Vec::new();
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let rest = self.pending.split_off(end + 1);
            let mut line = std::mem::replace(&mut self.pending, rest);
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            lines.push(String::from_utf8_lossy(&line).into_owned());
        }
        lines
    }

    /// The final, unterminated line, when the transport closed without a
    /// trailing newline. `None` if nothing is pending.
    pub(crate) fn flush(&mut self) -> Option<String> {
        if self.pending.is_empty() {
            return None;
        }
        let line = std::mem::take(&mut self.pending);
        Some(String::from_utf8_lossy(&line).into_owned())
    }
}

#[cfg(test)]
#[path = "tests/line_buffer.rs"]
mod tests;
