//! Reading what a file says it holds, without believing it.

use std::io::Read;

/// The most one entry of an archive may expand to: 256 MiB.
///
/// A story, a styles sheet, a document's JSON are megabytes at the very most;
/// a book-length story in an IDML is a few. A zip entry can say anything about
/// its own size, and a deflated one can expand a thousandfold — ten megabytes
/// of somebody else's file becomes ten gigabytes, and an allocation that size
/// ends the process, and every other open document's unsaved work with it.
/// The limit is far past anything real and far short of that.
pub const ENTRY_LIMIT: u64 = 256 * 1024 * 1024;

/// Read `reader` to the end as text, refusing at more than `limit` bytes.
///
/// An [`std::io::ErrorKind::InvalidData`] error when the limit is passed, with
/// nothing more read than the limit and one byte — the byte that proves it.
pub fn read_to_string_capped(reader: impl Read, limit: u64) -> std::io::Result<String> {
    let mut text = String::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_string(&mut text)?;
    if text.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("expands to more than {} MB", limit / (1024 * 1024)),
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_within_the_limit_reads_whole() {
        let text = read_to_string_capped("twelve bytes".as_bytes(), 12).expect("fits");
        assert_eq!(text, "twelve bytes");
    }

    #[test]
    fn text_past_the_limit_is_refused_after_one_byte_more() {
        let error = read_to_string_capped("thirteen byte".as_bytes(), 12).expect_err("too long");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn an_endless_reader_stops_at_the_limit() {
        // The zip bomb, in miniature: a source that would go on for ever.
        let error = read_to_string_capped(std::io::repeat(b'a'), 1024).expect_err("endless");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }
}
