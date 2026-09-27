//! Shared lexical paths and workspace-local link targets.

use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

/// Remove lexical `.` and `..` components without accessing the filesystem.
pub fn normalize(path: impl AsRef<Path>) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.as_ref().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match result.components().next_back() {
                Some(Component::Normal(_)) => {
                    result.pop();
                }
                None | Some(Component::ParentDir) => result.push(".."),
                _ => {}
            },
            _ => result.push(component.as_os_str()),
        }
    }
    result
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum LinkPathError {
    External,
    Template,
    OutsideWorkspace,
    Unknown,
}

pub(crate) struct LocalTarget {
    pub path: PathBuf,
    pub target: String,
}

pub(crate) struct LocalLink {
    pub location: LocalTarget,
    pub anchor: Option<String>,
}

/// Resolve a Markdown destination, including whether its anchor can be evaluated.
pub(crate) fn local_link(
    root: &Path,
    source: &Path,
    destination: &str,
) -> Result<LocalLink, LinkPathError> {
    let location = local_link_target(root, source, destination)?;
    if percent_decode(destination).is_some_and(|value| is_template(&value)) {
        return Err(LinkPathError::Template);
    }
    let anchor = destination
        .split_once('#')
        .map(|(_, value)| percent_decode(value).ok_or(LinkPathError::Unknown))
        .transpose()?;
    if anchor.as_deref().is_some_and(is_template) {
        return Err(LinkPathError::Template);
    }
    Ok(LocalLink { location, anchor })
}

/// Resolve only the target path. Anchor uncertainty does not make a known file absent.
pub(crate) fn local_link_target(
    root: &Path,
    source: &Path,
    destination: &str,
) -> Result<LocalTarget, LinkPathError> {
    if destination.starts_with("//") || has_scheme(destination) {
        return Err(LinkPathError::External);
    }
    let raw_path = destination.split(['?', '#']).next().unwrap_or_default();
    let path = percent_decode(raw_path).ok_or(LinkPathError::Unknown)?;
    if is_template(&path) {
        return Err(LinkPathError::Template);
    }
    if path.starts_with("//") || path.starts_with("\\\\") || has_scheme(&path) {
        return Err(LinkPathError::External);
    }
    let path = path.replace('\\', "/");
    let source = if source.is_absolute() {
        source.to_path_buf()
    } else {
        root.join(source)
    };
    let path = normalize(if path.is_empty() {
        source
    } else if path.starts_with('/') {
        root.join(path.trim_start_matches('/'))
    } else {
        source.parent().unwrap_or(root).join(path)
    });
    let root = normalize(root);
    let relative = path
        .strip_prefix(&root)
        .map_err(|_| LinkPathError::OutsideWorkspace)?;
    Ok(LocalTarget {
        target: relative.to_string_lossy().replace('\\', "/"),
        path,
    })
}

fn is_template(value: &str) -> bool {
    value.contains(['\0', '{', '}', '$', '<', '>'])
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

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum TargetStatus {
    File,
    Directory,
    Missing,
    Unknown,
    OutsideWorkspace,
    Unreadable(String),
}

/// Inspect the nearest existing ancestor before checking a possibly absent child.
/// This keeps missing children beneath outward symlinks outside the workspace.
pub(crate) fn local_target_status(root: &Path, target: &Path) -> TargetStatus {
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            return TargetStatus::Unreadable(format!("Cannot inspect workspace: {error}"));
        }
    };
    let mut contained = false;
    for ancestor in target.ancestors() {
        match ancestor.canonicalize() {
            Ok(actual) => {
                if !actual.starts_with(&root) {
                    return TargetStatus::OutsideWorkspace;
                }
                contained = true;
                break;
            }
            Err(error) if missing(&error) => {}
            Err(error) => {
                return TargetStatus::Unreadable(format!("Cannot resolve link target: {error}"));
            }
        }
    }
    if !contained {
        return TargetStatus::OutsideWorkspace;
    }
    match std::fs::metadata(target) {
        Ok(metadata) if metadata.is_dir() => TargetStatus::Directory,
        Ok(_) => TargetStatus::File,
        Err(error) if missing(&error) => TargetStatus::Missing,
        Err(error) => TargetStatus::Unreadable(format!("Cannot inspect link target: {error}")),
    }
}

fn missing(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_preserves_unresolved_relative_parents() {
        assert_eq!(normalize("../../a/../b"), PathBuf::from("../../b"));
        assert_eq!(normalize("a/../../b"), PathBuf::from("../b"));
        assert_eq!(normalize("a/./b/../c"), PathBuf::from("a/c"));
    }

    #[test]
    fn normalization_stops_at_the_filesystem_root() {
        let root = std::env::current_dir().unwrap();
        let filesystem_root: PathBuf = root
            .components()
            .take_while(|part| matches!(part, Component::Prefix(_) | Component::RootDir))
            .collect();
        assert_eq!(normalize(filesystem_root.join("../../..")), filesystem_root);
    }
}
