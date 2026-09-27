use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use seiso_diagnostics::Diagnostic;

use crate::CheckContext;

#[derive(Default)]
pub(crate) struct LinkResult {
    pub diagnostics: Vec<Diagnostic>,
    pub errors: Vec<String>,
    pub incomplete: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub enum PathStatus {
    Exists,
    Missing,
    Unknown,
    Error(String),
}

pub trait WorkspaceFiles {
    fn status(&self, workspace_root: &Path, target: &Path) -> PathStatus;
}

pub struct LocalWorkspaceFiles;

impl WorkspaceFiles for LocalWorkspaceFiles {
    fn status(&self, workspace_root: &Path, target: &Path) -> PathStatus {
        let root = match workspace_root.canonicalize() {
            Ok(root) => root,
            Err(error) => {
                return PathStatus::Error(format!(
                    "Cannot inspect workspace {} for link targets: {error}",
                    workspace_root.display()
                ));
            }
        };
        // An absent child of an external symlink is outside the workspace.
        for ancestor in target.ancestors() {
            match ancestor.canonicalize() {
                Ok(actual) => {
                    if !actual.starts_with(&root) {
                        return PathStatus::Unknown;
                    }
                    break;
                }
                Err(error) if missing(&error) => {}
                Err(error) => {
                    return PathStatus::Error(format!("Cannot resolve link target: {error}"));
                }
            }
        }
        match std::fs::metadata(target) {
            Ok(_) => PathStatus::Exists,
            Err(error) if missing(&error) => PathStatus::Missing,
            Err(error) => PathStatus::Error(format!("Cannot inspect link target: {error}")),
        }
    }
}

pub(crate) fn check(context: &CheckContext<'_>, files: &dyn WorkspaceFiles) -> LinkResult {
    let mut result = LinkResult::default();
    for link in &context.document.links {
        let destination = link.destination.as_str();
        let path_text = destination.split(['?', '#']).next().unwrap_or_default();
        if path_text.is_empty() || path_text.starts_with("//") || has_scheme(path_text) {
            continue;
        }
        let Some(decoded) = decode(path_text) else {
            result.incomplete = true;
            continue;
        };
        if decoded.contains(['\0', '{', '}', '$', '<', '>'])
            || decoded.starts_with("//")
            || decoded.starts_with("\\\\")
            || has_scheme(&decoded)
        {
            result.incomplete = true;
            continue;
        }
        let decoded = decoded.replace('\\', "/");
        let target = normalize(&if decoded.starts_with('/') {
            context.workspace_root.join(decoded.trim_start_matches('/'))
        } else {
            context
                .path
                .parent()
                .unwrap_or(context.workspace_root)
                .join(decoded)
        });
        if !target.starts_with(context.workspace_root) {
            result.incomplete = true;
            continue;
        }
        // The current document can be a new stdin overlay with no disk entry.
        if target == normalize(context.path) {
            continue;
        }
        match files.status(context.workspace_root, &target) {
            PathStatus::Exists => {}
            PathStatus::Missing => {
                result.diagnostics.push(Diagnostic::new(context.filename, &context.document.source,
                    "LNK001", link.span, format!("Relative link target {destination:?} does not exist in the workspace."),
                    "Update the relative path or restore the target file; check the path from this document's directory."));
            }
            PathStatus::Unknown => {
                result.incomplete = true;
            }
            PathStatus::Error(error) => {
                result.incomplete = true;
                result.errors.push(format!(
                    "Cannot inspect link target {destination:?}: {error}"
                ));
            }
        }
    }
    result.errors.sort();
    result.errors.dedup();
    result
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

fn decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = (*bytes.get(index + 1)? as char).to_digit(16)?;
            let low = (*bytes.get(index + 2)? as char).to_digit(16)?;
            result.push((high * 16 + low) as u8);
            index += 3;
        } else {
            result.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(result).ok()
}

fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(part.as_os_str()),
        }
    }
    normalized
}
