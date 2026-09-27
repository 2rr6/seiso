//! Edits for completed, unused suppression declarations.

use std::collections::BTreeSet;

use seiso_diagnostics::{Applicability, Diagnostic, Edit, Fix, SourceMap, Span};
use seiso_md::Document;

use crate::suppression::{SuppressionRecord, SuppressionState};

/// Attach one shared edit per declaration, using only visible stale diagnostics.
pub fn attach_fixes(
    document: &Document,
    suppressions: &[SuppressionRecord],
    diagnostics: &mut [Diagnostic],
) {
    for record in suppressions {
        if record.error.is_some() || record.scope.is_none() {
            continue;
        }
        let reported: BTreeSet<_> = diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code == "SUP002" && diagnostic.byte_range == record.span
            })
            .filter_map(|diagnostic| stale_code(diagnostic, record))
            .map(str::to_owned)
            .collect();
        if reported.is_empty() {
            continue;
        }
        let Some(edit) = declaration_edit(&document.source, record, &reported) else {
            continue;
        };
        let fix = Fix {
            message: "Remove unused suppression codes.".into(),
            applicability: Applicability::Safe,
            edits: vec![edit],
        };
        for diagnostic in diagnostics.iter_mut().filter(|diagnostic| {
            diagnostic.code == "SUP002"
                && diagnostic.byte_range == record.span
                && stale_code(diagnostic, record).is_some()
        }) {
            diagnostic.fix = Some(fix.clone());
        }
    }
}

fn stale_code<'a>(diagnostic: &Diagnostic, record: &'a SuppressionRecord) -> Option<&'a str> {
    // The suppression engine emits one diagnostic per code at the comment span.
    // Matching that code also preserves stale codes whose SUP002 was suppressed.
    record.codes.iter().find_map(|code| {
        (record.states.get(code) == Some(&SuppressionState::Stale)
            && diagnostic.message
                == format!("Suppression for {code} did not suppress any diagnostic"))
        .then_some(code.as_str())
    })
}

fn declaration_edit(
    source: &str,
    record: &SuppressionRecord,
    reported: &BTreeSet<String>,
) -> Option<Edit> {
    let comment = source.get(record.span.start..record.span.end)?;
    let body = comment.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let body = body.strip_prefix("seiso:")?.trim_start();
    let (command, arguments) = body.split_once(char::is_whitespace)?;
    if !matches!(command, "allow" | "allow-file") {
        return None;
    }
    let (codes, reason) = arguments.split_once("--")?;
    if reason.trim() != record.reason
        || codes.split(',').map(str::trim).collect::<Vec<_>>() != record.codes
    {
        return None;
    }
    let retained: Vec<_> = record
        .codes
        .iter()
        .filter(|code| !reported.contains(*code))
        .map(String::as_str)
        .collect();
    if !retained.is_empty() {
        let codes = codes.trim();
        let offset = codes.as_ptr() as usize - source.as_ptr() as usize;
        return Some(Edit {
            byte_range: Span::new(offset, offset + codes.len()),
            content: retained.join(", "),
        });
    }
    let start = source[..record.span.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let tail = &source[record.span.end..];
    let end = tail
        .find('\n')
        .map_or(source.len(), |index| record.span.end + index + 1);
    // A comment between nonblank lines can separate Markdown blocks. Leave an
    // empty line in that position instead of joining those blocks together.
    let separates_content = start > 0
        && !source[..start]
            .trim_end_matches(['\r', '\n'])
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .trim()
            .is_empty()
        && !source[end..]
            .split('\n')
            .next()
            .unwrap_or("")
            .trim()
            .is_empty();
    let range = if source[start..record.span.start].trim().is_empty()
        && source[record.span.end..end].trim().is_empty()
        && !separates_content
    {
        Span::new(start, end)
    } else {
        record.span
    };
    Some(Edit {
        byte_range: range,
        content: String::new(),
    })
}

/// Apply non-overlapping safe edits to the exact source snapshot that was checked.
/// The caller must separately compare the on-disk bytes before replacing a file.
pub fn apply_fixes(source: &str, diagnostics: &[Diagnostic]) -> Result<Option<String>, String> {
    let mut edits = BTreeSet::new();
    let mut filename = None;
    let source_map = SourceMap::new(source);
    for diagnostic in diagnostics {
        let Some(fix) = diagnostic
            .fix
            .as_ref()
            .filter(|fix| fix.applicability == Applicability::Safe)
        else {
            continue;
        };
        if let Some(previous) = filename.replace(&diagnostic.filename)
            && previous != &diagnostic.filename
        {
            return Err(
                "Cannot apply edits for multiple files to one source; group diagnostics by file."
                    .into(),
            );
        }
        validate_span(source, diagnostic.byte_range)?;
        if source_map.span_locations(diagnostic.byte_range)
            != (diagnostic.location, diagnostic.end_location)
        {
            return Err(
                "Diagnostic locations no longer match the source; run the check again.".into(),
            );
        }
        for edit in &fix.edits {
            validate_span(source, edit.byte_range)?;
            edits.insert(edit.clone());
        }
    }
    let edits: Vec<_> = edits.into_iter().collect();
    for pair in edits.windows(2) {
        if pair[1].byte_range.start < pair[0].byte_range.end
            || pair[0].byte_range.start == pair[1].byte_range.start
        {
            return Err("Safe edits overlap; run the check again before applying fixes.".into());
        }
    }
    if edits.is_empty() {
        return Ok(None);
    }
    let mut fixed = source.to_owned();
    for edit in edits.iter().rev() {
        fixed.replace_range(edit.byte_range.start..edit.byte_range.end, &edit.content);
    }
    Ok((fixed != source).then_some(fixed))
}

fn validate_span(source: &str, span: Span) -> Result<(), String> {
    if span.start > span.end || source.get(span.start..span.end).is_none() {
        return Err("Edit coordinates are outside the source or split a UTF-8 character; run the check again.".into());
    }
    Ok(())
}
