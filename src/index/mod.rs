//! Current workspace facts. Resolved paths and effective policy never enter the parse cache.

use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::config::Config;
use crate::diagnostics::Span;
use crate::md::{BlockKind, Document, FragmentKind};
use regex::Regex;
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct IndexedFile {
    pub filename: String,
    pub path: PathBuf,
    pub document: Arc<Document>,
    pub kind: Option<String>,
    pub domain: String,
    pub enabled_rules: Vec<String>,
    pub config: Config,
}

#[derive(Clone, Debug)]
pub struct WorkspaceIndex {
    pub root: PathBuf,
    pub files: Vec<IndexedFile>,
    pub complete: bool,
    anchors: BTreeMap<String, BTreeSet<String>>,
    anchor_spans: BTreeMap<String, BTreeMap<String, Span>>,
    inventory: Option<BTreeMap<String, InventoryEntryKind>>,
}

/// A complete, immutable file listing for evaluations without materializing upstream files.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryEntryKind {
    File,
    Directory,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkStatus {
    External,
    Template,
    OutsideWorkspace,
    Missing,
    Directory,
    File,
    AnchorFound,
    AnchorMissing,
    AnchorUnknown,
    Unreadable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LinkResolution {
    pub target: Option<String>,
    pub anchor: Option<String>,
    pub status: LinkStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl WorkspaceIndex {
    pub fn new(root: PathBuf, mut files: Vec<IndexedFile>, complete: bool) -> Self {
        files.sort_by(|left, right| left.filename.cmp(&right.filename));
        let anchor_spans: BTreeMap<_, _> = files
            .iter()
            .map(|file| (file.filename.clone(), document_anchors(&file.document)))
            .collect();
        let anchors = anchor_spans
            .iter()
            .map(|(filename, spans)| (filename.clone(), spans.keys().cloned().collect()))
            .collect();
        Self {
            root,
            files,
            complete,
            anchors,
            anchor_spans,
            inventory: None,
        }
    }

    pub fn with_inventory(mut self, inventory: BTreeMap<String, InventoryEntryKind>) -> Self {
        self.inventory = Some(inventory);
        self
    }

    pub fn file(&self, filename: &str) -> Option<&IndexedFile> {
        self.files
            .binary_search_by(|file| file.filename.as_str().cmp(filename))
            .ok()
            .map(|index| &self.files[index])
    }

    pub fn anchors(&self, filename: &str) -> Option<&BTreeSet<String>> {
        self.anchors.get(filename)
    }

    pub fn anchor_span(&self, filename: &str, anchor: &str) -> Option<Span> {
        self.anchor_spans.get(filename)?.get(anchor).copied()
    }

    /// Resolve against the current filesystem; indexed stdin overlays exist even without a disk file.
    pub fn resolve_link(&self, source: &str, destination: &str) -> LinkResolution {
        let mut result = LinkResolution {
            target: None,
            anchor: None,
            status: LinkStatus::AnchorUnknown,
            error: None,
        };
        if destination.starts_with("//") || has_scheme(destination) {
            result.status = LinkStatus::External;
            return result;
        }
        if percent_decode(destination)
            .is_some_and(|value| value.contains(['\0', '{', '}', '$', '<', '>']))
        {
            result.status = LinkStatus::Template;
            return result;
        }
        let (before_anchor, raw_anchor) = destination
            .split_once('#')
            .map_or((destination, None), |(path, anchor)| (path, Some(anchor)));
        let raw_path = before_anchor.split('?').next().unwrap_or_default();
        let Some(path) = percent_decode(raw_path) else {
            return result;
        };
        let anchor = match raw_anchor {
            Some(value) => match percent_decode(value) {
                Some(value) => Some(value),
                None => return result,
            },
            None => None,
        };
        if path.contains(['\0', '{', '}', '$', '<', '>'])
            || anchor
                .as_ref()
                .is_some_and(|value| value.contains(['\0', '{', '}', '$', '<', '>']))
        {
            result.status = LinkStatus::Template;
            return result;
        }
        if path.starts_with("//") || path.starts_with("\\\\") || has_scheme(&path) {
            result.status = LinkStatus::External;
            return result;
        }
        result.anchor = anchor;
        let path = path.replace('\\', "/");
        let target_path = normalize(&if path.is_empty() {
            self.root.join(source)
        } else if path.starts_with('/') {
            self.root.join(path.trim_start_matches('/'))
        } else {
            self.root
                .join(source)
                .parent()
                .unwrap_or(&self.root)
                .join(path)
        });
        let Ok(relative) = target_path.strip_prefix(&self.root) else {
            result.status = LinkStatus::OutsideWorkspace;
            return result;
        };
        let mut target = relative.to_string_lossy().replace('\\', "/");
        result.target = Some(target.clone());
        if let Some(inventory) = &self.inventory {
            let mut ancestor = Some(target.as_str());
            while let Some(path) = ancestor {
                if inventory.get(path) == Some(&InventoryEntryKind::Unknown) {
                    result.status = LinkStatus::AnchorUnknown;
                    return result;
                }
                ancestor = path.rsplit_once('/').map(|(parent, _)| parent);
            }
            result.status = if self.file(&target).is_some() {
                self.indexed_status(&target, result.anchor.as_deref())
            } else {
                match inventory.get(&target) {
                    Some(InventoryEntryKind::Directory) => LinkStatus::Directory,
                    Some(InventoryEntryKind::File)
                        if result.anchor.as_ref().is_none_or(String::is_empty) =>
                    {
                        LinkStatus::File
                    }
                    Some(_) => LinkStatus::AnchorUnknown,
                    None if target.is_empty() => LinkStatus::Directory,
                    None => LinkStatus::Missing,
                }
            };
            return result;
        }
        // Even a missing child beneath an outward symlink is outside the workspace.
        if let Err(status) = contained(&self.root, &target_path) {
            match status {
                ContainmentError::Outside => result.status = LinkStatus::OutsideWorkspace,
                ContainmentError::Io(error) => {
                    result.status = LinkStatus::Unreadable;
                    result.error = Some(error);
                }
            }
            return result;
        }
        // Resolve native aliases only through filesystem identity. Lowercasing names
        // would incorrectly merge distinct files on case-sensitive filesystems.
        if self.file(&target).is_none()
            && let (Ok(actual), Ok(root)) = (target_path.canonicalize(), self.root.canonicalize())
            && let Ok(relative) = actual.strip_prefix(root)
        {
            let canonical = relative.to_string_lossy().replace('\\', "/");
            if self.file(&canonical).is_some() {
                target = canonical;
                result.target = Some(target.clone());
            }
        }
        if self.file(&target).is_none() {
            match std::fs::metadata(&target_path) {
                Ok(metadata) if metadata.is_dir() => result.status = LinkStatus::Directory,
                Ok(_) => {
                    result.status = if result.anchor.as_ref().is_none_or(String::is_empty) {
                        LinkStatus::File
                    } else {
                        LinkStatus::AnchorUnknown
                    }
                }
                Err(error) if missing(&error) => result.status = LinkStatus::Missing,
                Err(error) => {
                    result.status = LinkStatus::Unreadable;
                    result.error = Some(format!("Cannot inspect link target {target:?}: {error}"));
                }
            }
        } else {
            result.status = self.indexed_status(&target, result.anchor.as_deref());
        }
        result
    }

    fn indexed_status(&self, target: &str, anchor: Option<&str>) -> LinkStatus {
        match anchor {
            None | Some("") => LinkStatus::File,
            Some(anchor) if self.anchors[target].contains(anchor) => LinkStatus::AnchorFound,
            Some(_) => LinkStatus::AnchorMissing,
        }
    }

    /// Stable debug output contains workspace-relative names, never runtime absolute paths.
    pub fn dump(&self) -> serde_json::Value {
        let files: Vec<_> = self.files.iter().map(|file| {
            let links: Vec<_> = file.document.links.iter().map(|link| serde_json::json!({
                "raw": link.destination,
                "span": link.span,
                "resolution": self.resolve_link(&file.filename, &link.destination),
            })).collect();
            serde_json::json!({
                "filename": file.filename,
                "kind": file.kind,
                "domain": file.domain,
                "language": file.document.language,
                "canonical": file.document.frontmatter.as_ref().filter(|value| value.errors.is_empty()).and_then(|value| value.canonical).unwrap_or(false),
                "anchors": self.anchor_spans[&file.filename],
                "identifiers": file.document.identifiers,
                "links": links,
            })
        }).collect();
        serde_json::json!({ "complete": self.complete, "files": files })
    }
}

/// Unicode category filtering follows github-slugger's documented generation rules.
pub fn github_slug(heading: &str) -> String {
    static STRIP: OnceLock<Regex> = OnceLock::new();
    let strip = STRIP.get_or_init(|| {
        Regex::new(
            r"[[\p{No}\p{Pe}\p{Pf}\p{Pi}\p{Ps}\p{Po}\p{Pd}\p{S}\p{C}\p{Z}]--[\p{Alphabetic} \-]]",
        )
        .expect("valid slug category expression")
    });
    strip
        .replace_all(&heading.to_lowercase(), "")
        .replace(' ', "-")
}

fn document_anchors(document: &Document) -> BTreeMap<String, Span> {
    let mut anchors = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for block in document
        .blocks
        .iter()
        .filter(|block| block.kind == BlockKind::Heading)
    {
        let text = document.sections[block.section]
            .heading
            .as_deref()
            .unwrap_or_default();
        let base = github_slug(text);
        let mut slug = base.clone();
        let mut suffix = 0;
        while !seen.insert(slug.clone()) {
            suffix += 1;
            slug = format!("{base}-{suffix}");
        }
        anchors.entry(slug).or_insert(block.span);
    }
    static TAG: OnceLock<Regex> = OnceLock::new();
    static ATTRIBUTE: OnceLock<Regex> = OnceLock::new();
    let tag = TAG.get_or_init(|| {
        Regex::new(r#"(?is)<([a-z][a-z0-9:-]*)\b(?:[^<>"']|"[^"]*"|'[^']*')*>"#).unwrap()
    });
    let attribute = ATTRIBUTE.get_or_init(|| {
        Regex::new(r#"(?is)\s+([^\s"'=<>`/]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?"#)
            .unwrap()
    });
    let mut ignored: Vec<Span> = document
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Code | BlockKind::HtmlComment))
        .map(|block| block.span)
        .collect();
    ignored.extend(document.comments.iter().map(|comment| comment.span));
    ignored.extend(document.frontmatter.iter().map(|value| value.span));
    ignored.extend(
        document
            .sentences
            .iter()
            .flat_map(|sentence| &sentence.fragments)
            .filter(|fragment| {
                matches!(
                    fragment.kind,
                    FragmentKind::InlineCode | FragmentKind::LinkDestination
                )
            })
            .map(|fragment| fragment.span),
    );
    let mut raw_text_end = 0;
    for capture in tag.captures_iter(&document.source) {
        let matched = capture.get(0).unwrap();
        let inside_html = document.blocks.iter().any(|block| {
            block.kind == BlockKind::Html
                && block.span.start <= matched.start()
                && matched.start() < block.span.end
        });
        if matched.start() < raw_text_end
            || ignored
                .iter()
                .any(|span| span.start <= matched.start() && matched.start() < span.end)
            || (!inside_html
                && document.source[..matched.start()]
                    .bytes()
                    .rev()
                    .take_while(|&byte| byte == b'\\')
                    .count()
                    % 2
                    != 0)
        {
            continue;
        }
        let element = capture[1].to_ascii_lowercase();
        if matches!(
            element.as_str(),
            "script"
                | "style"
                | "textarea"
                | "title"
                | "xmp"
                | "iframe"
                | "noembed"
                | "noframes"
                | "plaintext"
        ) {
            raw_text_end = document.source[matched.end()..]
                .to_ascii_lowercase()
                .find(&format!("</{element}"))
                .map_or(document.source.len(), |offset| matched.end() + offset);
        }
        for attr in attribute.captures_iter(matched.as_str()) {
            if !attr[1].eq_ignore_ascii_case("id")
                && !(attr[1].eq_ignore_ascii_case("name") && capture[1].eq_ignore_ascii_case("a"))
            {
                continue;
            }
            let Some(value) = attr.get(2).or_else(|| attr.get(3)).or_else(|| attr.get(4)) else {
                continue;
            };
            anchors
                .entry(decode_html(value.as_str()))
                .or_insert(Span::new(matched.start(), matched.end()));
        }
    }
    anchors
}

fn decode_html(value: &str) -> String {
    static ENTITY: OnceLock<Regex> = OnceLock::new();
    ENTITY
        .get_or_init(|| {
            Regex::new(r"&(#(?:x|X)[0-9a-fA-F]+|#[0-9]+|[a-zA-Z][a-zA-Z0-9]+);").unwrap()
        })
        .replace_all(value, |capture: &regex::Captures<'_>| {
            let entity = &capture[1];
            if let Some(number) = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
            {
                decode_numeric(number, 16)
            } else if let Some(number) = entity.strip_prefix('#') {
                decode_numeric(number, 10)
            } else {
                markdown::decode_named(entity, true).unwrap_or_else(|| capture[0].to_owned())
            }
        })
        .into_owned()
}

fn decode_numeric(number: &str, radix: u32) -> String {
    let codepoint = u32::from_str_radix(number, radix).unwrap_or(0xFFFD);
    // HTML's numeric-reference replacement table retains its Windows-1252 aliases.
    let codepoint = match codepoint {
        0 => 0xFFFD,
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        value => value,
    };
    char::from_u32(codepoint)
        .unwrap_or(char::REPLACEMENT_CHARACTER)
        .to_string()
}

enum ContainmentError {
    Outside,
    Io(String),
}

fn contained(root: &Path, target: &Path) -> Result<(), ContainmentError> {
    let root = root
        .canonicalize()
        .map_err(|error| ContainmentError::Io(format!("Cannot inspect workspace: {error}")))?;
    for ancestor in target.ancestors() {
        match ancestor.canonicalize() {
            Ok(actual) => {
                return if actual.starts_with(&root) {
                    Ok(())
                } else {
                    Err(ContainmentError::Outside)
                };
            }
            Err(error) if missing(&error) => {}
            Err(error) => {
                return Err(ContainmentError::Io(format!(
                    "Cannot resolve link target: {error}"
                )));
            }
        }
    }
    Err(ContainmentError::Outside)
}

fn missing(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory)
}

fn has_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once(':') else {
        return false;
    };
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            output.push(
                ((*bytes.get(index + 1)? as char).to_digit(16)? * 16
                    + (*bytes.get(index + 2)? as char).to_digit(16)?) as u8,
            );
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(root: &Path, filename: &str, source: &str) -> IndexedFile {
        IndexedFile {
            filename: filename.into(),
            path: root.join(filename),
            document: crate::md::parse(source).unwrap().into(),
            kind: Some("reference".into()),
            domain: String::new(),
            enabled_rules: Vec::new(),
            config: Config::defaults(root).unwrap(),
        }
    }

    #[test]
    fn github_anchors_cover_rendered_text_cjk_unicode_and_collision_suffixes() {
        let root = tempfile::tempdir().unwrap();
        let source = "# Hello, *world*!\n# Hello world\n# Hello world-1\n# Hello world\n# 安装：配置！\n# 日本語・ガイド\n# Déjà vu 🦀\n# A <em>formatted</em> `value`\n# Hello!  World\n# First. *Next*\n";
        let index = WorkspaceIndex::new(
            root.path().to_path_buf(),
            vec![file(root.path(), "a.md", source)],
            true,
        );
        let anchors = index.anchors("a.md").unwrap();
        for expected in [
            "hello-world",
            "hello-world-1",
            "hello-world-1-1",
            "hello-world-2",
            "安装配置",
            "日本語ガイド",
            "déjà-vu-",
            "a-formatted-value",
            "hello--world",
            "first-next",
        ] {
            assert!(
                anchors.contains(expected),
                "missing {expected:?} from {anchors:?}"
            );
        }
        assert_eq!(github_slug("one  two_name — x"), "one--two_name--x");
        assert!(index.anchor_span("a.md", "安装配置").is_some());
    }

    #[test]
    fn html_ids_and_names_exclude_comments_code_frontmatter_and_escaped_tags() {
        let root = tempfile::tempdir().unwrap();
        let source = "---\nnote: '<a id=frontmatter>'\n---\n# Title\n\n<a name='old'></a> <span ID=custom></span> <a id=two&amp;three></a>\n\n<div id=block></div>\n\n`<a id=inline>`\n\n```html\n<a id=fenced>\n```\n\n<!-- <a id=comment> -->\n\n\\<a id=escaped>\n\n<span name=not-an-anchor></span>\n\n<span title='the id=quoted'></span>\n";
        let index = WorkspaceIndex::new(
            root.path().to_path_buf(),
            vec![file(root.path(), "a.md", source)],
            true,
        );
        let anchors = index.anchors("a.md").unwrap();
        for expected in ["title", "old", "custom", "two&three", "block"] {
            assert!(anchors.contains(expected), "{expected}: {anchors:?}");
        }
        for absent in [
            "frontmatter",
            "inline",
            "fenced",
            "comment",
            "escaped",
            "not-an-anchor",
            "quoted",
        ] {
            assert!(!anchors.contains(absent), "{absent}: {anchors:?}");
        }
    }

    #[test]
    fn links_decode_paths_and_anchors_and_keep_excluded_targets_unknown() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("docs")).unwrap();
        std::fs::write(root.path().join("excluded.md"), "# Known only on disk").unwrap();
        std::fs::write(root.path().join("image.png"), "image").unwrap();
        let index = WorkspaceIndex::new(
            root.path().to_path_buf(),
            vec![
                file(root.path(), "docs/from.md", "# From"),
                file(
                    root.path(),
                    "hello world.md",
                    "---\nkind: generated\n---\n# 标题\n",
                ),
            ],
            true,
        );
        for (destination, expected) in [
            (
                "../hello%20world.md#%E6%A0%87%E9%A2%98",
                LinkStatus::AnchorFound,
            ),
            ("/hello%20world.md#missing", LinkStatus::AnchorMissing),
            ("../excluded.md#whatever", LinkStatus::AnchorUnknown),
            ("../image.png#fragment", LinkStatus::AnchorUnknown),
            ("../image.png", LinkStatus::File),
            ("../docs", LinkStatus::Directory),
            ("../missing.md#missing", LinkStatus::Missing),
            ("../../../external.md#x", LinkStatus::OutsideWorkspace),
            ("../${name}.md", LinkStatus::Template),
            ("../hello%20world.md#{{anchor}}", LinkStatus::Template),
            (
                "../hello%20world.md?q=%7Bname%7D#missing",
                LinkStatus::Template,
            ),
            ("https://example.com/#x", LinkStatus::External),
            ("//example.com/#x", LinkStatus::External),
            ("#from", LinkStatus::AnchorFound),
            ("?view=1#from", LinkStatus::AnchorFound),
            ("#", LinkStatus::File),
            ("%ZZ.md", LinkStatus::AnchorUnknown),
        ] {
            assert_eq!(
                index.resolve_link("docs/from.md", destination).status,
                expected,
                "{destination}"
            );
        }
        assert_eq!(
            index
                .resolve_link("docs/from.md", "../hello%20world.md#标题")
                .target
                .as_deref(),
            Some("hello world.md")
        );
    }

    #[test]
    fn moved_source_recomputes_links_and_dump_is_order_independent() {
        let root = tempfile::tempdir().unwrap();
        let first = file(root.path(), "a.md", "[to](target.md#yes)");
        let moved = file(root.path(), "sub/a.md", &first.document.source);
        let target = file(root.path(), "target.md", "# Yes");
        let index = WorkspaceIndex::new(
            root.path().to_path_buf(),
            vec![first.clone(), moved.clone(), target.clone()],
            true,
        );
        assert_eq!(
            index.resolve_link("a.md", "target.md#yes").status,
            LinkStatus::AnchorFound
        );
        assert_eq!(
            index.resolve_link("sub/a.md", "target.md#yes").status,
            LinkStatus::Missing
        );
        let reversed =
            WorkspaceIndex::new(root.path().to_path_buf(), vec![target, moved, first], true);
        assert_eq!(index.dump(), reversed.dump());
        assert!(
            !index
                .dump()
                .to_string()
                .contains(root.path().to_str().unwrap())
        );
    }

    #[test]
    fn raw_html_text_is_not_parsed_as_nested_elements_and_entities_decode() {
        let root = tempfile::tempdir().unwrap();
        let source = "<script id=script>const example = '<a id=fake>';</script>\n\n<a id='&copy;&#xE9;&#233;'></a>\n\n<textarea><a id=also-fake></textarea>\n";
        let index = WorkspaceIndex::new(
            root.path().to_path_buf(),
            vec![file(root.path(), "a.md", source)],
            true,
        );
        let anchors = index.anchors("a.md").unwrap();
        assert!(anchors.contains("script"));
        assert!(anchors.contains("©éé"));
        assert!(!anchors.contains("fake"));
        assert!(!anchors.contains("also-fake"));
        assert_eq!(decode_html("&#128;&#x110000;&#99999999999999999;"), "€��");
    }

    #[test]
    fn inventory_matches_disk_resolution_and_never_reads_unknown_subtrees() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("docs")).unwrap();
        std::fs::write(root.path().join("asset.png"), "image").unwrap();
        std::fs::write(root.path().join("excluded.md"), "# Excluded").unwrap();
        let files = vec![
            file(root.path(), "docs/from.md", "# From"),
            file(root.path(), "target.md", "# 标题"),
        ];
        let disk = WorkspaceIndex::new(root.path().to_path_buf(), files.clone(), true);
        let inventory = WorkspaceIndex::new(root.path().to_path_buf(), files, true).with_inventory(
            BTreeMap::from([
                ("docs".into(), InventoryEntryKind::Directory),
                ("asset.png".into(), InventoryEntryKind::File),
                ("excluded.md".into(), InventoryEntryKind::File),
                ("submodule".into(), InventoryEntryKind::Unknown),
            ]),
        );
        for destination in [
            "#from",
            "../target.md#标题",
            "/target.md#absent",
            "../asset.png",
            "../asset.png#x",
            "../excluded.md#x",
            "/docs",
            "/",
            "../../outside.md",
            "../missing.md",
            "../target.md?q={{name}}",
        ] {
            assert_eq!(
                disk.resolve_link("docs/from.md", destination),
                inventory.resolve_link("docs/from.md", destination),
                "{destination}"
            );
        }
        assert_eq!(
            inventory
                .resolve_link("docs/from.md", "../submodule/missing.md#x")
                .status,
            LinkStatus::AnchorUnknown
        );
        // Virtual snapshots use their inventory even when the workspace root does not exist.
        let absent = root.path().join("absent-workspace");
        let virtual_index = WorkspaceIndex::new(absent, Vec::new(), true).with_inventory(
            BTreeMap::from([("file.md".into(), InventoryEntryKind::File)]),
        );
        assert_eq!(
            virtual_index.resolve_link("from.md", "file.md").status,
            LinkStatus::File
        );
        assert_eq!(
            virtual_index.resolve_link("from.md", "missing.md").status,
            LinkStatus::Missing
        );
    }

    #[test]
    fn invalid_frontmatter_cannot_claim_canonical_ownership_in_dump() {
        let root = tempfile::tempdir().unwrap();
        let source = "---\nkind: [reference]\ncanonical: true\n---\n# Fields\n";
        let file = file(root.path(), "a.md", source);
        assert!(
            !file
                .document
                .frontmatter
                .as_ref()
                .unwrap()
                .errors
                .is_empty()
        );
        let index = WorkspaceIndex::new(root.path().to_path_buf(), vec![file], true);
        assert_eq!(index.dump()["files"][0]["canonical"], false);
    }

    #[cfg(unix)]
    #[test]
    fn links_below_external_symlinks_remain_unknown_even_when_target_is_missing() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("outside")).unwrap();
        let index = WorkspaceIndex::new(
            root.path().to_path_buf(),
            vec![file(root.path(), "a.md", "# A")],
            true,
        );
        assert_eq!(
            index.resolve_link("a.md", "outside/missing.md#x").status,
            LinkStatus::OutsideWorkspace
        );
    }
}
