use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use seiso_diagnostics::Span;
use seiso_md::{Document, MarkdownFlavor, ParseOptions, parse_with_options};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Serialize)]
struct FlavorResult {
    flavor: MarkdownFlavor,
    source_sha256: String,
    sha256: String,
    serialized_bytes: u64,
    language: seiso_md::Language,
    sections: usize,
    blocks: usize,
    sentences: usize,
    links: usize,
    frontmatter_errors: usize,
}

#[derive(Serialize)]
struct Outcome {
    status: &'static str,
    flavors: Vec<FlavorResult>,
    error: Option<String>,
}

struct HashWriter {
    hash: Sha256,
    bytes: u64,
}

impl Write for HashWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.hash.update(buffer);
        self.bytes += buffer.len() as u64;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn validate(document: &Document) -> Result<(), String> {
    let source = &document.source;
    let span = |range: Span| -> Result<(), String> {
        if range.start > range.end || source.get(range.start..range.end).is_none() {
            return Err(format!("invalid source byte range {range:?}"));
        }
        Ok(())
    };
    if document.sections.is_empty()
        || document.sections[0].parent.is_some()
        || document.sections[0].span != Span::new(0, source.len())
    {
        return Err("document root section is missing or incomplete".into());
    }
    if let Some(frontmatter) = &document.frontmatter {
        span(frontmatter.span)?;
        for error in &frontmatter.errors {
            span(error.span)?;
        }
    }
    for (index, section) in document.sections.iter().enumerate() {
        span(section.span)?;
        if let Some(heading) = section.heading_span {
            span(heading)?;
        }
        if let Some(parent) = section.parent
            && (parent >= index || !document.sections[parent].children.contains(&index))
        {
            return Err(format!("section {index} has an invalid parent"));
        }
        for &child in &section.children {
            if document
                .sections
                .get(child)
                .is_none_or(|item| item.parent != Some(index))
            {
                return Err(format!("section {index} has an invalid child"));
            }
        }
        for &block in &section.blocks {
            if document
                .blocks
                .get(block)
                .is_none_or(|item| item.section != index)
            {
                return Err(format!("section {index} has an invalid block"));
            }
        }
    }
    for (index, block) in document.blocks.iter().enumerate() {
        span(block.span)?;
        if document
            .sections
            .get(block.section)
            .is_none_or(|section| !section.blocks.contains(&index))
        {
            return Err(format!("block {index} has an invalid section"));
        }
        if let Some(parent) = block.parent
            && (parent >= index || !document.blocks[parent].children.contains(&index))
        {
            return Err(format!("block {index} has an invalid parent"));
        }
        for &child in &block.children {
            if document
                .blocks
                .get(child)
                .is_none_or(|item| item.parent != Some(index))
            {
                return Err(format!("block {index} has an invalid child"));
            }
        }
        for &sentence in &block.sentences {
            if document
                .sentences
                .get(sentence)
                .is_none_or(|item| item.block != index)
            {
                return Err(format!("block {index} has an invalid sentence"));
            }
        }
    }
    for (index, sentence) in document.sentences.iter().enumerate() {
        span(sentence.span)?;
        if document
            .blocks
            .get(sentence.block)
            .is_none_or(|block| !block.sentences.contains(&index))
        {
            return Err(format!("sentence {index} has an invalid block"));
        }
        for fragment in &sentence.fragments {
            span(fragment.span)?;
            let mut end = 0;
            for mapping in &fragment.mapping {
                span(mapping.source)?;
                if mapping.text.start != end
                    || mapping.text.start >= mapping.text.end
                    || fragment
                        .text
                        .get(mapping.text.start..mapping.text.end)
                        .is_none()
                {
                    return Err(format!(
                        "sentence {index} has an invalid decoded-text mapping"
                    ));
                }
                end = mapping.text.end;
            }
            if end != fragment.text.len() {
                return Err(format!(
                    "sentence {index} has an incomplete decoded-text mapping"
                ));
            }
        }
    }
    for link in &document.links {
        span(link.span)?;
        span(link.destination_span)?;
    }
    for comment in &document.comments {
        span(comment.span)?;
    }
    for identifier in &document.identifiers {
        span(identifier.span)?;
        if document
            .blocks
            .get(identifier.block)
            .is_none_or(|block| block.section != identifier.section)
        {
            return Err("identifier has an invalid block or section".into());
        }
    }
    Ok(())
}

fn inspect(source: &str, flavor: MarkdownFlavor) -> Result<FlavorResult, String> {
    let document =
        parse_with_options(source, ParseOptions { flavor }).map_err(|error| error.to_string())?;
    if document.source != source {
        return Err("the document model changed the original source".into());
    }
    validate(&document)?;
    let mut output = HashWriter {
        hash: Sha256::new(),
        bytes: 0,
    };
    serde_json::to_writer(&mut output, &document).map_err(|error| error.to_string())?;
    Ok(FlavorResult {
        flavor,
        source_sha256: format!("{:x}", Sha256::digest(source.as_bytes())),
        sha256: format!("{:x}", output.hash.finalize()),
        serialized_bytes: output.bytes,
        language: document.language,
        sections: document.sections.len(),
        blocks: document.blocks.len(),
        sentences: document.sentences.len(),
        links: document.links.len(),
        frontmatter_errors: document
            .frontmatter
            .as_ref()
            .map_or(0, |frontmatter| frontmatter.errors.len()),
    })
}

fn run() -> Outcome {
    let mut args = std::env::args_os().skip(1);
    let Some(path) = args.next().map(PathBuf::from) else {
        return Outcome {
            status: "input_error",
            flavors: Vec::new(),
            error: Some("expected one Markdown file path".into()),
        };
    };
    if args.next().is_some() {
        return Outcome {
            status: "input_error",
            flavors: Vec::new(),
            error: Some("expected exactly one Markdown file path".into()),
        };
    }
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            return Outcome {
                status: "input_error",
                flavors: Vec::new(),
                error: Some(error.to_string()),
            };
        }
    };
    let mut flavors = Vec::new();
    for flavor in [MarkdownFlavor::CommonMark, MarkdownFlavor::Gfm] {
        let first = match inspect(&source, flavor) {
            Ok(result) => result,
            Err(error) => {
                return Outcome {
                    status: "model_error",
                    flavors,
                    error: Some(format!("{flavor:?}: {error}")),
                };
            }
        };
        let second = match inspect(&source, flavor) {
            Ok(result) => result,
            Err(error) => {
                return Outcome {
                    status: "model_error",
                    flavors,
                    error: Some(format!("{flavor:?}: {error}")),
                };
            }
        };
        if first.sha256 != second.sha256 || first.serialized_bytes != second.serialized_bytes {
            return Outcome {
                status: "nondeterministic",
                flavors,
                error: Some(format!("{flavor:?}: repeated serialization differs")),
            };
        }
        flavors.push(first);
    }
    Outcome {
        status: "ok",
        flavors,
        error: None,
    }
}

fn main() -> ExitCode {
    let outcome = std::panic::catch_unwind(run).unwrap_or_else(|error| {
        let message = error
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                error
                    .downcast_ref::<&str>()
                    .map(|message| (*message).to_owned())
            })
            .unwrap_or_else(|| "non-string panic payload".into());
        Outcome {
            status: "panic",
            flavors: Vec::new(),
            error: Some(message),
        }
    });
    let success = outcome.status == "ok";
    if serde_json::to_writer(io::stdout().lock(), &outcome).is_err() {
        return ExitCode::from(2);
    }
    ExitCode::from(if success { 0 } else { 1 })
}
