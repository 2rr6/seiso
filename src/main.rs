use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use seiso::config::Workspace;
use seiso::md::Document;
use seiso::rules::{KindResolution, resolve_kind};
use serde::Serialize;

mod commands;

#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser)]
#[command(
    name = "seiso",
    version,
    about = "A Markdown convention and linter for project docs written by AI and read by humans and agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check Markdown and report documentation diagnostics.
    Check(commands::CheckArgs),
    /// Print the effective file policies as deterministic JSON.
    Policy(commands::PolicyArgs),
    /// Inspect headings, links, and file roles in the current workspace index.
    Index(commands::IndexArgs),
    /// Print a rule's explanation and examples.
    Rule(commands::RuleArgs),
    /// Create a configuration with suggested kind mappings.
    Init,
    /// Adapt editor events to Markdown checks.
    Hook {
        #[command(subcommand)]
        command: commands::HookCommand,
    },
    /// Parse Markdown into the seiso document model (does not run lint rules).
    Parse(ParseArgs),
}

#[derive(clap::Args)]
struct ParseArgs {
    /// Files or directories to inspect; defaults to the workspace.
    paths: Vec<PathBuf>,
    /// Use this configuration for every selected file.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Output format. JSON contains the full document model.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    output_format: OutputFormat,
    /// Read stdin in place of this workspace file; never writes to disk.
    #[arg(long, value_name = "PATH", conflicts_with = "paths")]
    stdin_filename: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Serialize)]
struct ParsedFile {
    filename: String,
    configuration: Option<String>,
    kind: KindResolution,
    domain: Option<String>,
    document: Document,
}

#[derive(Serialize)]
struct InputError {
    filename: String,
    message: String,
}

#[derive(Default, Serialize)]
struct ParseReport {
    files: Vec<ParsedFile>,
    errors: Vec<InputError>,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            for line in error.lines() {
                eprintln!(
                    "seiso: {}",
                    line.replace('\r', "\\r").replace("##[", "## [")
                );
            }
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<u8, String> {
    match cli.command {
        Command::Parse(args) => parse_workspace(args),
        Command::Check(args) => commands::check(args),
        Command::Policy(args) => commands::policy(args),
        Command::Index(args) => commands::index(args),
        Command::Rule(args) => commands::rule(args),
        Command::Init => commands::init(),
        Command::Hook { command } => Ok(commands::hook(command)),
    }
}

fn parse_workspace(args: ParseArgs) -> Result<u8, String> {
    let cwd = std::env::current_dir()
        .map_err(|error| format!("Cannot determine the current directory: {error}"))?;
    let workspace =
        Workspace::discover(&cwd, args.config.as_deref()).map_err(|error| error.to_string())?;
    let root = normalize(&workspace.root);
    let mut report = ParseReport::default();
    let mut inputs = BTreeMap::<PathBuf, Option<String>>::new();

    if let Some(path) = &args.stdin_filename {
        let path = absolute(&cwd, path);
        require_workspace_path(&root, &path)?;
        if !is_markdown(&path) {
            return Err("The stdin filename must have a .md or .markdown extension.".into());
        }
        if ignored_by_git(&root, &path)? {
            return Err(format!(
                "{} is excluded by .gitignore; choose an included Markdown path.",
                relative(&root, &path)
            ));
        }
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .map_err(|error| format!("Cannot read UTF-8 Markdown from stdin: {error}"))?;
        inputs.insert(path, Some(source));
    } else {
        let mut requested = BTreeSet::new();
        for path in &args.paths {
            let path = absolute(&cwd, path);
            require_workspace_path(&root, &path)?;
            match path.try_exists() {
                Ok(true) => {
                    requested.insert(identity(&path));
                }
                Ok(false) => report.errors.push(InputError {
                    filename: relative(&root, &path),
                    message: "Path does not exist; provide an existing file or directory.".into(),
                }),
                Err(error) => report.errors.push(InputError {
                    filename: relative(&root, &path),
                    message: format!("Cannot access this path: {error}"),
                }),
            }
        }
        let walker = ignore::WalkBuilder::new(&root)
            .hidden(false)
            .follow_links(false)
            .git_ignore(true)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .parents(false)
            .require_git(false)
            .filter_entry(|entry| entry.file_name() != ".git")
            .build();
        for entry in walker {
            match entry {
                Ok(entry) => {
                    let path = normalize(entry.path());
                    if let Some(error) = entry.error() {
                        report.errors.push(InputError {
                            filename: relative(&root, &path),
                            message: format!("Cannot fully apply ignore rules: {error}"),
                        });
                    }
                    if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                        continue;
                    }
                    if !is_markdown(&path) {
                        continue;
                    }
                    if !args.paths.is_empty() {
                        let candidate = identity(&path);
                        if !requested
                            .iter()
                            .any(|selected| candidate.starts_with(selected))
                        {
                            continue;
                        }
                    }
                    inputs.insert(path, None);
                }
                Err(error) => report.errors.push(InputError {
                    filename: ".".into(),
                    message: format!("Cannot finish workspace discovery: {error}"),
                }),
            }
        }
    }

    for (path, replacement) in inputs {
        let filename = relative(&root, &path);
        let config = match workspace.config_for(&path) {
            Ok(config) => config,
            Err(error) => {
                report.errors.push(InputError {
                    filename,
                    message: error.to_string(),
                });
                continue;
            }
        };
        if !config.includes(&path) || config.excludes(&path) {
            if replacement.is_some() {
                report.errors.push(InputError {
                    filename,
                    message: "The stdin filename is excluded by configuration; choose an included Markdown path.".into(),
                });
            }
            continue;
        }
        let source = match replacement.map_or_else(|| std::fs::read_to_string(&path), Ok) {
            Ok(source) => source,
            Err(error) => {
                report.errors.push(InputError {
                    filename,
                    message: format!("Cannot read UTF-8 Markdown: {error}"),
                });
                continue;
            }
        };
        match seiso::md::parse(&source) {
            Ok(document) => report.files.push(ParsedFile {
                configuration: config.source.as_ref().map(|path| relative(&root, path)),
                kind: resolve_kind(&document, config.kind_for(&path)),
                domain: config.domain_for(&path).map(str::to_owned),
                filename,
                document,
            }),
            Err(error) => report.errors.push(InputError {
                filename,
                message: format!("Cannot parse Markdown: {error}"),
            }),
        }
    }
    report.files.sort_by(|a, b| a.filename.cmp(&b.filename));
    report
        .errors
        .sort_by(|a, b| (&a.filename, &a.message).cmp(&(&b.filename, &b.message)));
    report
        .errors
        .dedup_by(|a, b| a.filename == b.filename && a.message == b.message);

    let rendered = match args.output_format {
        OutputFormat::Json => {
            serde_json::to_string_pretty(&report)
                .map_err(|error| format!("Cannot encode the parse report: {error}"))?
                + "\n"
        }
        OutputFormat::Text => render_summary(&report),
    };
    io::stdout()
        .lock()
        .write_all(rendered.as_bytes())
        .map_err(|error| format!("Cannot write the parse report: {error}"))?;
    for error in &report.errors {
        eprintln!("{}: {}", error.filename, error.message);
    }
    Ok(if report.errors.is_empty() { 0 } else { 2 })
}

fn render_summary(report: &ParseReport) -> String {
    use std::fmt::Write as _;
    let mut output = String::new();
    for file in &report.files {
        let _ = writeln!(
            output,
            "{}: kind={} ({}), sections={}, blocks={}, sentences={}",
            file.filename,
            file.kind.value.as_deref().unwrap_or("unknown"),
            file.kind.source,
            file.document.sections.len().saturating_sub(1),
            file.document.blocks.len(),
            file.document.sentences.len()
        );
        if let Some(problem) = &file.kind.problem {
            let _ = writeln!(output, "  {problem}");
        }
    }
    let _ = writeln!(
        output,
        "Parsed {} file(s). No lint rules were run.",
        report.files.len()
    );
    output
}

fn absolute(cwd: &Path, path: &Path) -> PathBuf {
    normalize(&if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    })
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        })
}

fn ignored_by_git(root: &Path, path: &Path) -> Result<bool, String> {
    let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
    let components = relative.components().collect::<Vec<_>>();
    let mut directory = root.to_path_buf();
    let mut matchers = Vec::new();
    for (index, component) in components.iter().enumerate() {
        if component.as_os_str() == ".git" {
            return Ok(true);
        }
        let ignore_file = directory.join(".gitignore");
        if ignore_file.is_file() {
            let mut builder = ignore::gitignore::GitignoreBuilder::new(&directory);
            if let Some(error) = builder.add(&ignore_file) {
                return Err(format!("Cannot read {}: {error}", ignore_file.display()));
            }
            matchers.push(
                builder
                    .build()
                    .map_err(|error| format!("Cannot read Git ignore patterns: {error}"))?,
            );
        }
        directory.push(component.as_os_str());
        let is_directory = index + 1 < components.len();
        for matcher in matchers.iter().rev() {
            let matched = matcher.matched(&directory, is_directory);
            if matched.is_ignore() {
                return Ok(true);
            }
            if matched.is_whitelist() {
                break;
            }
        }
    }
    Ok(false)
}

fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

fn require_workspace_path(root: &Path, path: &Path) -> Result<(), String> {
    workspace_path(root, path).map(|_| ())
}

/// Resolve existing aliases, including the parent of a new stdin document.
fn workspace_path(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("Cannot resolve workspace {}: {error}", root.display()))?;
    for ancestor in path.ancestors() {
        let resolved = match ancestor.canonicalize() {
            Ok(resolved) => resolved,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("Cannot resolve {}: {error}", path.display())),
        };
        let suffix = path
            .strip_prefix(ancestor)
            .map_err(|error| error.to_string())?;
        if !suffix.as_os_str().is_empty() && !ancestor.is_dir() {
            return Err(format!(
                "{} is not a directory; choose a valid workspace path.",
                ancestor.display()
            ));
        }
        let resolved = resolved.join(suffix);
        return resolved
            .strip_prefix(&canonical_root)
            .map(|relative| root.join(relative))
            .map_err(|_| {
                format!(
                    "{} is outside workspace {}; run from its workspace directory.",
                    path.display(),
                    root.display()
                )
            });
    }
    Err(format!(
        "Cannot resolve {} to an existing workspace directory.",
        path.display()
    ))
}

fn identity(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| normalize(path))
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
