use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand, ValueEnum};
use seiso_config::{CliOverrides, Settings, Workspace};
use seiso_diagnostics::{Diagnostic, render_concise, render_json, render_text};
use serde::Serialize;

use super::{
    InputError, absolute, identity, ignored_by_git, is_markdown, normalize, relative,
    require_workspace_path,
};

#[derive(Args, Default)]
pub struct SelectionArgs {
    /// Enable selected preview rules.
    #[arg(long)]
    preview: bool,
    /// Replace configured selectors; use comma-separated codes or families.
    #[arg(long, value_delimiter = ',')]
    select: Option<Vec<String>>,
    /// Add comma-separated codes or families to the configured selection.
    #[arg(long, value_delimiter = ',')]
    extend_select: Vec<String>,
}

impl SelectionArgs {
    fn overrides(&self) -> CliOverrides {
        CliOverrides {
            preview: self.preview,
            select: self.select.clone(),
            extend_select: self.extend_select.clone(),
        }
    }
}

#[derive(Args)]
pub struct CheckArgs {
    /// Files or directories to check; defaults to the workspace.
    paths: Vec<PathBuf>,
    /// Use this configuration for every selected file.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(flatten)]
    selection: SelectionArgs,
    /// Diagnostic output format.
    #[arg(long, value_enum, default_value_t = CheckFormat::Text)]
    output_format: CheckFormat,
    /// Read stdin in place of this workspace file; never writes to disk.
    #[arg(long, value_name = "PATH", conflicts_with = "paths")]
    stdin_filename: Option<PathBuf>,
    /// Return success for violations; incomplete checks still return 2.
    #[arg(long)]
    exit_zero: bool,
    /// Run without a cache (checks currently always read the current files).
    #[arg(long)]
    no_cache: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum CheckFormat {
    Text,
    Concise,
    Json,
}

#[derive(Args)]
pub struct PolicyArgs {
    /// Use this configuration for every file.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(flatten)]
    selection: SelectionArgs,
}

#[derive(Args)]
pub struct RuleArgs {
    #[arg(required_unless_present = "all", conflicts_with = "all")]
    code: Option<String>,
    /// Print documentation for every implemented rule.
    #[arg(long)]
    all: bool,
}

#[derive(Subcommand)]
pub enum HookCommand {
    /// Read a Claude Code PostToolUse event from stdin.
    ClaudeCode {
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
        #[command(flatten)]
        selection: SelectionArgs,
    },
}

#[derive(Serialize)]
struct FilePolicy {
    filename: String,
    configuration: String,
    kind: Option<seiso_rules::KindResolution>,
    domain: Option<String>,
    enabled_rules: Vec<String>,
    excluded: Option<&'static str>,
    suppressions: serde_json::Value,
}

#[derive(Default, Serialize)]
struct PolicyReport {
    configurations: BTreeMap<String, Settings>,
    files: Vec<FilePolicy>,
    errors: Vec<InputError>,
}

#[derive(Default)]
struct Evaluation {
    diagnostics: Vec<Diagnostic>,
    sources: BTreeMap<String, String>,
    policy: PolicyReport,
    enabled_count: usize,
}

impl Evaluation {
    fn exit_code(&self, exit_zero: bool) -> u8 {
        if !self.policy.errors.is_empty() {
            2
        } else if !exit_zero && !self.diagnostics.is_empty() {
            1
        } else {
            0
        }
    }

    fn error(&mut self, filename: impl Into<String>, message: impl Into<String>) {
        self.policy.errors.push(InputError {
            filename: filename.into(),
            message: message.into(),
        });
    }

    fn render(&self, format: CheckFormat) -> Result<String, String> {
        match format {
            CheckFormat::Text => Ok(render_text(&self.diagnostics, &self.sources)),
            CheckFormat::Concise => Ok(render_concise(&self.diagnostics)),
            CheckFormat::Json => render_json(&self.diagnostics).map_err(|error| error.to_string()),
        }
    }

    fn print_errors(&self) {
        for error in &self.policy.errors {
            eprintln!("{}: {}", error.filename, error.message);
        }
    }
}

pub fn check(args: CheckArgs) -> Result<u8, String> {
    let cwd = current_dir()?;
    let replacement = args
        .stdin_filename
        .as_ref()
        .map(|_| read_stdin())
        .transpose()?;
    let evaluation = evaluate(&cwd, &args, replacement, false)?;
    write_stdout(&evaluation.render(args.output_format)?)?;
    evaluation.print_errors();
    if evaluation.enabled_count == 0 && evaluation.policy.errors.is_empty() {
        eprintln!(
            "seiso: No rules enabled for the selected files. Inspect `seiso policy` for the effective selection and file kinds."
        );
    }
    Ok(evaluation.exit_code(args.exit_zero))
}

pub fn policy(args: PolicyArgs) -> Result<u8, String> {
    let args = CheckArgs {
        paths: Vec::new(),
        config: args.config,
        selection: args.selection,
        output_format: CheckFormat::Json,
        stdin_filename: None,
        exit_zero: false,
        no_cache: true,
    };
    let evaluation = evaluate(&current_dir()?, &args, None, true)?;
    let mut output =
        serde_json::to_string_pretty(&evaluation.policy).map_err(|error| error.to_string())?;
    output.push('\n');
    write_stdout(&output)?;
    evaluation.print_errors();
    Ok(if evaluation.policy.errors.is_empty() {
        0
    } else {
        2
    })
}

fn evaluate(
    cwd: &Path,
    args: &CheckArgs,
    replacement: Option<String>,
    policy_mode: bool,
) -> Result<Evaluation, String> {
    let overrides = args.selection.overrides();
    overrides.validate().map_err(|error| error.to_string())?;
    let workspace =
        Workspace::discover(cwd, args.config.as_deref()).map_err(|error| error.to_string())?;
    let root = normalize(&workspace.root);
    let mut evaluation = Evaluation::default();
    let mut requested = BTreeSet::new();
    let mut inputs = BTreeMap::new();
    if let Some(path) = &args.stdin_filename {
        let path = absolute(cwd, path);
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
        inputs.insert(path, replacement);
    } else {
        for path in &args.paths {
            let path = absolute(cwd, path);
            require_workspace_path(&root, &path)?;
            match path.try_exists() {
                Ok(true) => {
                    requested.insert(identity(&path));
                }
                Ok(false) => evaluation.error(
                    relative(&root, &path),
                    "Path does not exist; provide an existing file or directory.",
                ),
                Err(error) => evaluation.error(
                    relative(&root, &path),
                    format!("Cannot access this path: {error}"),
                ),
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
                        evaluation.error(
                            relative(&root, &path),
                            format!("Cannot fully apply ignore rules: {error}"),
                        );
                    }
                    if !entry.file_type().is_some_and(|kind| kind.is_file()) || !is_markdown(&path)
                    {
                        continue;
                    }
                    if !args.paths.is_empty()
                        && !requested
                            .iter()
                            .any(|selected| identity(&path).starts_with(selected))
                    {
                        continue;
                    }
                    inputs.insert(path, None);
                }
                Err(error) => {
                    evaluation.error(".", format!("Cannot finish workspace discovery: {error}"))
                }
            }
        }
    }
    // Include the workspace configuration even when no Markdown file is selected.
    record_configuration(&mut evaluation, &root, &workspace.config, &overrides);
    for (path, source_override) in inputs {
        let filename = relative(&root, &path);
        let config = match workspace.config_for(&path) {
            Ok(config) => config,
            Err(error) => {
                evaluation.error(filename, error.to_string());
                continue;
            }
        };
        let configuration = record_configuration(&mut evaluation, &root, &config, &overrides);
        let excluded = if !config.includes(&path) {
            Some("include")
        } else if config.excludes(&path) {
            Some("exclude")
        } else {
            None
        };
        if excluded.is_some() {
            if source_override.is_some() {
                evaluation.error(&filename, "The stdin filename is excluded by configuration; choose an included Markdown path.");
            }
            if policy_mode {
                evaluation.policy.files.push(FilePolicy {
                    filename,
                    configuration,
                    kind: None,
                    domain: config.domain_for(&path).map(str::to_owned),
                    enabled_rules: Vec::new(),
                    excluded,
                    suppressions: serde_json::json!([]),
                });
            }
            continue;
        }
        let source = match source_override.map_or_else(|| fs::read_to_string(&path), Ok) {
            Ok(source) => source,
            Err(error) => {
                evaluation.error(filename, format!("Cannot read UTF-8 Markdown: {error}"));
                continue;
            }
        };
        let document = match seiso_md::parse(&source) {
            Ok(document) => document,
            Err(error) => {
                evaluation.error(filename, format!("Cannot parse Markdown: {error}"));
                continue;
            }
        };
        let context = seiso_rules::CheckContext {
            document: &document,
            filename: &filename,
            path: &path,
            workspace_root: &root,
            config: &config,
            overrides: &overrides,
        };
        let result = match seiso_rules::check(&context) {
            Ok(result) => result,
            Err(error) => {
                evaluation.error(filename, error.to_string());
                continue;
            }
        };
        for error in result.errors {
            evaluation.error(&filename, error);
        }
        evaluation.enabled_count += result.enabled_rules.len();
        evaluation.diagnostics.extend(result.diagnostics);
        evaluation.sources.insert(filename.clone(), source);
        evaluation.policy.files.push(FilePolicy {
            filename,
            configuration,
            kind: Some(result.kind),
            domain: config.domain_for(&path).map(str::to_owned),
            enabled_rules: result.enabled_rules,
            excluded: None,
            suppressions: serde_json::to_value(result.suppressions)
                .map_err(|error| error.to_string())?,
        });
    }
    evaluation
        .policy
        .files
        .sort_by(|a, b| a.filename.cmp(&b.filename));
    evaluation
        .policy
        .errors
        .sort_by(|a, b| (&a.filename, &a.message).cmp(&(&b.filename, &b.message)));
    evaluation
        .policy
        .errors
        .dedup_by(|a, b| a.filename == b.filename && a.message == b.message);
    Ok(evaluation)
}

fn record_configuration(
    evaluation: &mut Evaluation,
    root: &Path,
    config: &seiso_config::Config,
    overrides: &CliOverrides,
) -> String {
    let name = config
        .source
        .as_ref()
        .map(|path| relative(root, path))
        .unwrap_or_else(|| "<defaults>".into());
    let mut settings = config.settings.clone();
    settings.preview |= overrides.preview;
    if let Some(select) = &overrides.select {
        settings.lint.select.clone_from(select);
    }
    settings.lint.select.extend(overrides.extend_select.clone());
    evaluation
        .policy
        .configurations
        .insert(name.clone(), settings);
    name
}

pub fn rule(args: RuleArgs) -> Result<u8, String> {
    let docs = if args.all {
        seiso_rules::rules()
            .iter()
            .map(render_rule)
            .collect::<Vec<_>>()
            .join("\n\n")
    } else {
        let code = args.code.as_deref().unwrap_or_default();
        let rule = seiso_rules::rule(code).ok_or_else(|| {
            format!(
                "Rule {code:?} is not implemented; run `seiso rule --all` to list available rules."
            )
        })?;
        render_rule(rule)
    };
    write_stdout(&(docs + "\n"))?;
    Ok(0)
}

fn render_rule(rule: &seiso_rules::Rule) -> String {
    let mut output = String::new();
    let mut status_added = false;
    for line in rule.documentation.trim_end().lines() {
        output.push_str(line);
        output.push('\n');
        if !status_added && line.starts_with("# ") {
            output.push_str(&format!("\nStatus: {}.\n", rule.status()));
            status_added = true;
        }
    }
    output.trim_end().to_owned()
}

pub fn init() -> Result<u8, String> {
    let cwd = current_dir()?;
    let workspace = Workspace::discover(&cwd, None).map_err(|error| error.to_string())?;
    if let Some(path) = workspace.config.source {
        return Err(format!(
            "Configuration already exists at {}; edit that file instead.",
            path.display()
        ));
    }
    let mut contents = String::from(
        "include = [\"**/*.md\", \"**/*.markdown\"]\npreview = false\n\n# Review these path mappings and declare other kinds in document frontmatter.\n",
    );
    for (path, kind, exists) in [
        ("**/README.md", "readme", cwd.join("README.md").is_file()),
        (
            "**/CHANGELOG.md",
            "changelog",
            cwd.join("CHANGELOG.md").is_file(),
        ),
        ("docs/guides/**", "howto", cwd.join("docs/guides").is_dir()),
        ("docs/howto/**", "howto", cwd.join("docs/howto").is_dir()),
        (
            "docs/reference/**",
            "reference",
            cwd.join("docs/reference").is_dir(),
        ),
        (
            "docs/runbooks/**",
            "runbook",
            cwd.join("docs/runbooks").is_dir(),
        ),
        ("docs/adr/**", "adr", cwd.join("docs/adr").is_dir()),
        ("docs/plans/**", "plan", cwd.join("docs/plans").is_dir()),
    ] {
        if exists {
            contents.push_str(&format!(
                "\n[[kinds]]\npath = \"{path}\"\nkind = \"{kind}\"\n"
            ));
        }
    }
    let path = cwd.join("seiso.toml");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| {
            format!(
                "Cannot create {}: {error}; preserve or edit the existing configuration.",
                path.display()
            )
        })?;
    file.write_all(contents.as_bytes())
        .map_err(|error| format!("Cannot write {}: {error}", path.display()))?;
    write_stdout(
        "Created seiso.toml. Review the suggested kind mappings, then run `seiso check`.\n",
    )?;
    Ok(0)
}

pub fn hook(command: HookCommand) -> u8 {
    match run_hook(command) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("seiso: {error}");
            1
        }
    }
}

fn run_hook(command: HookCommand) -> Result<u8, String> {
    let HookCommand::ClaudeCode { config, selection } = command;
    let input: serde_json::Value = serde_json::from_str(&read_stdin()?).map_err(|error| {
        format!(
            "Cannot read Claude Code hook JSON: {error}; configure a PostToolUse Write|Edit hook."
        )
    })?;
    let cwd = input
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("Claude Code hook input needs a non-empty cwd path.")?;
    let cwd = PathBuf::from(cwd);
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err("Claude Code hook cwd must be an existing absolute directory.".into());
    }
    let path = input.get("tool_input").and_then(|value| value.get("file_path"))
        .and_then(serde_json::Value::as_str).filter(|value| !value.is_empty())
        .ok_or("Claude Code hook input needs tool_input.file_path; use a PostToolUse Write|Edit matcher.")?;
    let path = PathBuf::from(path);
    if !is_markdown(&path) {
        return Ok(0);
    }
    let args = CheckArgs {
        paths: vec![path],
        config,
        selection,
        output_format: CheckFormat::Concise,
        stdin_filename: None,
        exit_zero: false,
        no_cache: true,
    };
    let evaluation = evaluate(&cwd, &args, None, false)?;
    match evaluation.exit_code(false) {
        0 => Ok(0),
        1 => {
            io::stderr()
                .lock()
                .write_all(evaluation.render(CheckFormat::Concise)?.as_bytes())
                .map_err(|error| format!("Cannot write hook diagnostics: {error}"))?;
            Ok(2)
        }
        _ => {
            io::stderr()
                .lock()
                .write_all(evaluation.render(CheckFormat::Concise)?.as_bytes())
                .map_err(|error| format!("Cannot write hook diagnostics: {error}"))?;
            evaluation.print_errors();
            Ok(1)
        }
    }
}

fn current_dir() -> Result<PathBuf, String> {
    std::env::current_dir()
        .map_err(|error| format!("Cannot determine the current directory: {error}"))
}

fn read_stdin() -> Result<String, String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| format!("Cannot read UTF-8 stdin: {error}"))?;
    Ok(input)
}

fn write_stdout(text: &str) -> Result<(), String> {
    io::stdout()
        .lock()
        .write_all(text.as_bytes())
        .map_err(|error| format!("Cannot write output: {error}"))
}
