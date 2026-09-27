use std::collections::BTreeSet;

use seiso::diagnostics::{Applicability, Diagnostic, Edit, Fix, Span};
use seiso::md::parse;
use seiso::rules::{
    fixes::{apply_fixes, attach_fixes},
    suppression::{SuppressionResult, apply},
};

fn codes(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn check(
    source: &str,
    diagnostics: Vec<Diagnostic>,
    enabled: &[&str],
    incomplete: &[&str],
) -> SuppressionResult {
    let document = parse(source).unwrap();
    let mut result = apply(
        &document,
        "guide.md",
        diagnostics,
        &codes(enabled),
        &codes(incomplete),
    );
    attach_fixes(&document, &result.suppressions, &mut result.diagnostics);
    result
}

#[test]
fn removes_only_stale_codes_and_preserves_other_states_and_reason() {
    let source = "<!-- seiso: allow LNK001, LNK002, STL001, PTR001 -- External conventions. -->\r\n\r\nTarget.";
    let start = source.find("Target").unwrap();
    let active = Diagnostic::new(
        "guide.md",
        source,
        "LNK002",
        Span::new(start, start + 6),
        "Problem",
        "Fix",
    );
    let result = check(
        source,
        vec![active],
        &["LNK001", "LNK002", "STL001", "SUP002"],
        &["STL001"],
    );
    assert_eq!(result.diagnostics.len(), 1);
    let fixed = apply_fixes(source, &result.diagnostics).unwrap().unwrap();
    assert_eq!(fixed, source.replacen("LNK001, ", "", 1));
    let again = check(&fixed, vec![], &["LNK001", "STL001", "SUP002"], &["STL001"]);
    assert_eq!(apply_fixes(&fixed, &again.diagnostics).unwrap(), None);
}

#[test]
fn standalone_comments_remove_the_line_with_its_original_newline() {
    for newline in ["\n", "\r\n"] {
        let source = format!(
            "---{newline}kind: howto{newline}---{newline}  <!-- seiso: allow-file LNK001 -- Historical. --> \t{newline}{newline}正文。"
        );
        let result = check(&source, vec![], &["LNK001", "SUP002"], &[]);
        let fixed = apply_fixes(&source, &result.diagnostics).unwrap().unwrap();
        assert_eq!(
            fixed,
            format!("---{newline}kind: howto{newline}---{newline}{newline}正文。")
        );
        let again = check(&fixed, vec![], &["LNK001", "SUP002"], &[]);
        assert_eq!(apply_fixes(&fixed, &again.diagnostics).unwrap(), None);
    }
}

#[test]
fn inline_paragraph_list_and_table_preserve_every_surrounding_byte() {
    let comment = "<!-- seiso: allow LNK001 -- Historical. -->";
    for source in [
        format!("Before {comment} after."),
        format!("中文{comment}English"),
        format!("- Before {comment} after.\r\n- Next item."),
        format!("| Before {comment} after | Other |\n| --- | --- |\n| A | B |"),
    ] {
        let result = check(&source, vec![], &["LNK001", "SUP002"], &[]);
        let fixed = apply_fixes(&source, &result.diagnostics).unwrap().unwrap();
        assert_eq!(fixed, source.replace(comment, ""));
    }
}

#[test]
fn removing_a_comment_between_paragraphs_preserves_the_block_boundary() {
    let source = "First paragraph.\n<!-- seiso: allow LNK001 -- Historical. -->\nSecond paragraph.";
    let result = check(source, vec![], &["LNK001", "SUP002"], &[]);
    let fixed = apply_fixes(source, &result.diagnostics).unwrap().unwrap();
    assert_eq!(fixed, "First paragraph.\n\nSecond paragraph.");
    assert_eq!(
        parse(&fixed)
            .unwrap()
            .blocks
            .iter()
            .filter(|block| block.kind == seiso::md::BlockKind::Paragraph)
            .count(),
        2
    );
}

#[test]
fn multiple_stale_diagnostics_share_one_deduplicated_edit() {
    let source = "<!-- seiso: allow LNK001, PTR001 -- Historical. -->\n\nTarget.";
    let result = check(source, vec![], &["LNK001", "PTR001", "SUP002"], &[]);
    assert_eq!(result.diagnostics.len(), 2);
    assert_eq!(result.diagnostics[0].fix, result.diagnostics[1].fix);
    assert_eq!(
        apply_fixes(source, &result.diagnostics).unwrap().unwrap(),
        "\nTarget."
    );
}

#[test]
fn suppressed_unused_codes_and_meta_declarations_are_preserved() {
    let source = "<!-- seiso: allow-file SUP002 -- Preserve historical exemptions. -->\n\n<!-- seiso: allow LNK001 -- Historical. -->\n\nTarget.";
    let result = check(source, vec![], &["LNK001", "SUP002"], &[]);
    assert!(result.diagnostics.is_empty());
    assert_eq!(apply_fixes(source, &result.diagnostics).unwrap(), None);

    let document = parse("<!-- seiso: allow LNK001, PTR001 -- Historical. -->\n\nTarget.").unwrap();
    let mut result = apply(
        &document,
        "guide.md",
        vec![],
        &codes(&["LNK001", "PTR001", "SUP002"]),
        &BTreeSet::new(),
    );
    result
        .diagnostics
        .retain(|diagnostic| diagnostic.message.contains("LNK001"));
    attach_fixes(&document, &result.suppressions, &mut result.diagnostics);
    assert_eq!(
        apply_fixes(&document.source, &result.diagnostics)
            .unwrap()
            .unwrap(),
        "<!-- seiso: allow PTR001 -- Historical. -->\n\nTarget."
    );
}

#[test]
fn invalid_disabled_incomplete_and_code_examples_have_no_edits() {
    for (source, enabled, incomplete) in [
        (
            "<!-- seiso: allow LNK001 -- Missing target. -->",
            vec!["LNK001", "SUP001", "SUP002"],
            vec![],
        ),
        (
            "<!-- seiso: allow LNK001 -- Disabled. -->\n\nTarget.",
            vec!["SUP002"],
            vec![],
        ),
        (
            "<!-- seiso: allow LNK001 -- Incomplete. -->\n\nTarget.",
            vec!["LNK001", "SUP002"],
            vec!["LNK001"],
        ),
        (
            "```markdown\n<!-- seiso: allow LNK001 -- Example. -->\n```\n\nTarget.",
            vec!["LNK001", "SUP002"],
            vec![],
        ),
        (
            "`<!-- seiso: allow LNK001 -- Example. -->`\n\nTarget.",
            vec!["LNK001", "SUP002"],
            vec![],
        ),
    ] {
        let result = check(source, vec![], &enabled, &incomplete);
        assert_eq!(
            apply_fixes(source, &result.diagnostics).unwrap(),
            None,
            "{source}"
        );
    }
}

fn edit_diagnostic(source: &str, span: Span, content: &str) -> Diagnostic {
    let mut diagnostic = Diagnostic::new(
        "guide.md",
        source,
        "SUP002",
        Span::new(0, 0),
        "Problem",
        "Fix",
    );
    diagnostic.fix = Some(Fix {
        applicability: Applicability::Safe,
        message: "Fix".into(),
        edits: vec![Edit {
            byte_range: span,
            content: content.into(),
        }],
    });
    diagnostic
}

#[test]
fn rejects_overlap_conflicting_insertions_and_invalid_utf8_coordinates() {
    let source = "中文 text";
    let first = edit_diagnostic(source, Span::new(0, 6), "");
    let overlap = edit_diagnostic(source, Span::new(3, 7), "");
    assert!(apply_fixes(source, &[first, overlap]).is_err());
    for span in [Span::new(0, 1), Span::new(3, 99), Span::new(6, 3)] {
        assert!(apply_fixes(source, &[edit_diagnostic(source, span, "")]).is_err());
    }
    let insertions = [
        edit_diagnostic(source, Span::new(0, 0), "a"),
        edit_diagnostic(source, Span::new(0, 0), "b"),
    ];
    assert!(apply_fixes(source, &insertions).is_err());
    let mut stale = edit_diagnostic(source, Span::new(0, 3), "");
    stale.location.column = 2;
    assert!(apply_fixes(source, &[stale]).is_err());
}

#[test]
fn applies_disjoint_edits_in_reverse_and_ignores_unsafe_edits() {
    let source = "one two three";
    let first = edit_diagnostic(source, Span::new(0, 3), "1");
    let last = edit_diagnostic(source, Span::new(8, 13), "3");
    let mut unsafe_edit = edit_diagnostic(source, Span::new(4, 7), "2");
    unsafe_edit.fix.as_mut().unwrap().applicability = Applicability::Unsafe;
    assert_eq!(
        apply_fixes(source, &[last, unsafe_edit, first])
            .unwrap()
            .unwrap(),
        "1 two 3"
    );
}
