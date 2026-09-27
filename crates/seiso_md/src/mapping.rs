use crate::{Fragment, FragmentKind, SourceSegment};
use seiso_diagnostics::Span;

pub(crate) fn fragment(source: &str, kind: FragmentKind, text: &str, span: Span) -> Fragment {
    let mapping = map_text(source, text, span, kind == FragmentKind::InlineCode);
    Fragment {
        kind,
        text: text.to_owned(),
        span,
        mapping,
    }
}

fn map_text(source: &str, text: &str, span: Span, code: bool) -> Vec<SourceSegment> {
    if text.is_empty() {
        return Vec::new();
    }
    let raw = &source[span.start..span.end];
    let mut left = 0;
    let mut right = raw.len();
    if code {
        left = raw.bytes().take_while(|byte| *byte == b'`').count();
        right = raw
            .len()
            .saturating_sub(raw.bytes().rev().take_while(|byte| *byte == b'`').count());
        if left > right {
            return coarse(text, span);
        }
        let normalized = raw[left..right]
            .replace("\r\n", " ")
            .replace(['\r', '\n'], " ");
        if normalized.starts_with(' ')
            && normalized.ends_with(' ')
            && normalized.chars().any(|ch| ch != ' ')
        {
            left += if raw[left..right].starts_with("\r\n") {
                2
            } else {
                1
            };
            right -= if raw[left..right].ends_with("\r\n") {
                2
            } else {
                1
            };
        }
    }
    let mut source_cursor = left;
    let mut text_cursor = 0;
    let mut segments = Vec::new();
    let mut line_start = false;
    while source_cursor < right && text_cursor < text.len() {
        let rest = &raw[source_cursor..right];
        let expected = &text[text_cursor..];
        let Some(ch) = rest.chars().next() else { break };
        let mut size = ch.len_utf8();
        let decoded = if ch == '\r' || ch == '\n' {
            if rest.starts_with("\r\n") {
                size = 2;
            }
            if code {
                " ".to_owned()
            } else if expected.starts_with(&rest[..size]) {
                rest[..size].to_owned()
            } else {
                "\n".to_owned()
            }
        } else if !code && ch == '\\' {
            if let Some(next) = rest[1..].chars().next().filter(char::is_ascii_punctuation) {
                size += next.len_utf8();
                next.to_string()
            } else {
                ch.to_string()
            }
        } else if !code && ch == '&' {
            if let Some((entity, length)) = entity(rest) {
                size = length;
                entity
            } else {
                ch.to_string()
            }
        } else if ch == '\0' {
            "�".to_owned()
        } else {
            ch.to_string()
        };

        if expected.starts_with(&decoded) {
            // One reference may expand to multiple characters, all from the same syntax.
            for (offset, decoded_char) in decoded.char_indices() {
                segments.push(SourceSegment {
                    text: Span {
                        start: text_cursor + offset,
                        end: text_cursor + offset + decoded_char.len_utf8(),
                    },
                    source: Span {
                        start: span.start + source_cursor,
                        end: span.start + source_cursor + size,
                    },
                    exact: true,
                });
            }
            text_cursor += decoded.len();
            line_start = ch == '\r' || ch == '\n';
        } else if !(line_start && (ch == '>' || ch == ' ' || ch == '\t')) {
            return coarse(text, span);
        }
        source_cursor += size;
    }
    if text_cursor != text.len() || !raw[source_cursor..right].trim().is_empty() {
        return coarse(text, span);
    }
    segments
}

fn coarse(text: &str, source: Span) -> Vec<SourceSegment> {
    vec![SourceSegment {
        text: Span {
            start: 0,
            end: text.len(),
        },
        source,
        exact: false,
    }]
}

fn entity(raw: &str) -> Option<(String, usize)> {
    let end = raw.find(';').filter(|end| *end <= 33)?;
    let body = &raw[1..end];
    let decoded = if let Some(hex) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
        if hex.is_empty() || hex.len() > 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        markdown::decode_numeric(hex, 16)
    } else if let Some(decimal) = body.strip_prefix('#') {
        if decimal.is_empty()
            || decimal.len() > 7
            || !decimal.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        markdown::decode_numeric(decimal, 10)
    } else {
        markdown::decode_named(body, true)?
    };
    Some((decoded, end + 1))
}

pub(crate) fn slice_fragment(fragment: &Fragment, start: usize, end: usize) -> Fragment {
    let mapping: Vec<_> = fragment
        .mapping
        .iter()
        .filter_map(|segment| {
            let left = segment.text.start.max(start);
            let right = segment.text.end.min(end);
            (left < right).then(|| SourceSegment {
                text: Span {
                    start: left - start,
                    end: right - start,
                },
                source: segment.source,
                exact: segment.exact,
            })
        })
        .collect();
    let span = mapping
        .iter()
        .fold(None::<Span>, |range, segment| {
            Some(match range {
                None => segment.source,
                Some(range) => Span {
                    start: range.start.min(segment.source.start),
                    end: range.end.max(segment.source.end),
                },
            })
        })
        .unwrap_or(fragment.span);
    Fragment {
        kind: fragment.kind,
        text: fragment.text[start..end].to_owned(),
        span,
        mapping,
    }
}
