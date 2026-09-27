use crate::{Location, Span};

/// Maps original source byte offsets to one-based Unicode scalar locations.
/// Offsets inside a UTF-8 character move to its start; offsets past EOF move to
/// EOF. Both bytes of CRLF belong to the preceding line's end column.
#[derive(Debug)]
pub struct SourceMap<'a> {
    source: &'a str,
    lines: Vec<Span>,
}

impl<'a> SourceMap<'a> {
    pub fn new(source: &'a str) -> Self {
        let mut lines = Vec::new();
        let mut start = 0;
        let mut offset = 0;
        let bytes = source.as_bytes();
        while offset < bytes.len() {
            match bytes[offset] {
                b'\r' | b'\n' => {
                    lines.push(Span::new(start, offset));
                    if bytes[offset] == b'\r' && bytes.get(offset + 1) == Some(&b'\n') {
                        offset += 1;
                    }
                    start = offset + 1;
                }
                _ => {}
            }
            offset += 1;
        }
        lines.push(Span::new(start, source.len()));
        Self { source, lines }
    }

    pub fn location(&self, offset: usize) -> Location {
        let offset = self.clamp_offset(offset);
        let index = self.lines.partition_point(|line| line.start <= offset) - 1;
        let line = self.lines[index];
        Location {
            row: index + 1,
            column: self.source[line.start..offset.min(line.end)]
                .chars()
                .count()
                + 1,
        }
    }

    pub fn span_locations(&self, span: Span) -> (Location, Location) {
        let span = self.clamp_span(span);
        (self.location(span.start), self.location(span.end))
    }

    /// Returns a line without its line ending; `row` is one-based.
    pub fn line(&self, row: usize) -> Option<&'a str> {
        let span = self.lines.get(row.checked_sub(1)?)?;
        Some(&self.source[span.start..span.end])
    }

    pub fn clamp_span(&self, span: Span) -> Span {
        let start = self.clamp_offset(span.start);
        let end = self.clamp_offset(span.end).max(start);
        Span::new(start, end)
    }

    fn clamp_offset(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.source.len());
        while !self.source.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_count_unicode_scalars_in_original_markdown() {
        let source = "# Hello 中文 日本語 🦀\n\\*escaped\\* &amp; e\u{301}";
        let map = SourceMap::new(source);
        for (needle, row, column) in [
            ("Hello", 1, 3),
            ("中文", 1, 9),
            ("日本語", 1, 12),
            ("🦀", 1, 16),
            ("escaped", 2, 3),
            ("&amp;", 2, 13),
            ("\u{301}", 2, 20),
        ] {
            assert_eq!(
                map.location(source.find(needle).unwrap()),
                Location { row, column }
            );
        }
        assert_eq!(map.location(source.len()), Location { row: 2, column: 21 });
    }

    #[test]
    fn crlf_and_bare_cr_each_start_one_new_line() {
        let source = "中\r\n日\rA\n";
        let map = SourceMap::new(source);
        for (offset, row, column) in [
            (0, 1, 1),
            (3, 1, 2),
            (4, 1, 2),
            (5, 2, 1),
            (8, 2, 2),
            (9, 3, 1),
            (10, 3, 2),
            (11, 4, 1),
        ] {
            assert_eq!(map.location(offset), Location { row, column });
        }
        assert_eq!(map.line(0), None);
        assert_eq!(map.line(1), Some("中"));
        assert_eq!(map.line(2), Some("日"));
        assert_eq!(map.line(3), Some("A"));
        assert_eq!(map.line(4), Some(""));
        assert_eq!(map.line(5), None);
    }

    #[test]
    fn arbitrary_offsets_and_reversed_ranges_cannot_panic() {
        for source in ["", "中", "🦀\r\n日本\r", "a\n"] {
            let map = SourceMap::new(source);
            for start in 0..source.len() + 5 {
                for end in 0..source.len() + 5 {
                    let span = map.clamp_span(Span::new(start, end));
                    assert!(source.get(span.start..span.end).is_some());
                    let (first, last) = map.span_locations(Span::new(start, end));
                    assert!(first <= last);
                    assert!(map.line(first.row).is_some());
                }
            }
            assert_eq!(map.location(usize::MAX), map.location(source.len()));
            assert!(map.clamp_span(Span::new(usize::MAX, 0)).is_empty());
        }
    }
}
