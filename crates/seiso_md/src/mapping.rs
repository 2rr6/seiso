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
    // Most text is verbatim. One linear range preserves every character offset.
    if &raw[left..right] == text {
        return vec![SourceSegment {
            text: Span::new(0, text.len()),
            source: Span::new(span.start + left, span.start + right),
            exact: true,
        }];
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
                let segment = SourceSegment {
                    text: Span {
                        start: text_cursor + offset,
                        end: text_cursor + offset + decoded_char.len_utf8(),
                    },
                    source: Span {
                        start: span.start + source_cursor,
                        end: span.start + source_cursor + size,
                    },
                    exact: true,
                };
                append_segment(&mut segments, segment);
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

fn append_segment(segments: &mut Vec<SourceSegment>, segment: SourceSegment) {
    if let Some(previous) = segments.last_mut()
        && previous.exact
        && segment.exact
        && previous.text.end == segment.text.start
        && previous.source.end == segment.source.start
        && previous.text.end - previous.text.start == previous.source.end - previous.source.start
        && segment.text.end - segment.text.start == segment.source.end - segment.source.start
    {
        previous.text.end = segment.text.end;
        previous.source.end = segment.source.end;
    } else {
        segments.push(segment);
    }
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
                source: segment.covered_source(&fragment.text, Span::new(left, right)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_verbatim_maps_preserve_every_unicode_byte_and_slice() {
        let text = "English 中文 日本語 🦀 text";
        let source = format!("before {text} after");
        let value = fragment(
            &source,
            FragmentKind::Text,
            text,
            Span::new(7, 7 + text.len()),
        );
        assert_eq!(value.mapping.len(), 1);
        for (offset, ch) in text.char_indices() {
            for byte in offset..offset + ch.len_utf8() {
                assert_eq!(
                    value.source_span(Span::new(byte, byte + 1)),
                    Some(Span::new(7 + offset, 7 + offset + ch.len_utf8()))
                );
            }
        }
        let start = text.find('中').unwrap();
        let end = text.find('🦀').unwrap() + '🦀'.len_utf8();
        let sliced = slice_fragment(&value, start, end);
        assert_eq!(sliced.span, Span::new(7 + start, 7 + end));
        assert_eq!(sliced.mapping.len(), 1);
        assert_eq!(
            sliced.source_span(Span::new(0, '中'.len_utf8())),
            Some(Span::new(7 + start, 7 + start + '中'.len_utf8()))
        );
    }

    #[test]
    fn compact_mapping_keeps_escapes_and_multi_character_entities_indivisible() {
        let source = "中文 &amp; \\* &NotEqualTilde; finish";
        let text = "中文 & * ≂̸ finish";
        let value = fragment(source, FragmentKind::Text, text, Span::new(0, source.len()));
        for (token, original) in [
            ("中文", "中文"),
            ("&", "&amp;"),
            ("*", "\\*"),
            ("≂", "&NotEqualTilde;"),
            ("̸", "&NotEqualTilde;"),
            ("finish", "finish"),
        ] {
            let start = text.find(token).unwrap();
            let expected = source.find(original).unwrap();
            assert_eq!(
                value.source_span(Span::new(start, start + token.len())),
                Some(Span::new(expected, expected + original.len()))
            );
        }
        let start = text.find("finish").unwrap();
        let sliced = slice_fragment(&value, start, text.len());
        assert_eq!(&source[sliced.span.start..sliced.span.end], "finish");
        assert_eq!(
            sliced.source_span(Span::new(1, 4)),
            Some(Span::new(source.len() - 5, source.len() - 2))
        );
    }
}
