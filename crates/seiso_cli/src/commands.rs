use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Args, Subcommand, ValueEnum};
use seiso_config::{CliOverrides, Settings, Workspace};
use seiso_diagnostics::{
    Diagnostic, render_concise, render_github, render_json, render_sarif, render_text,
};
use seiso_index::{IndexedFile, WorkspaceIndex};
use serde::Serialize;

use super::{
    InputError, absolute, ignored_by_git, is_markdown, normalize, relative, workspace_path,
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
    /// Read and parse every source without reading or writing the parse cache.
    #[arg(long)]
    no_cache: bool,
    /// Apply verified safe fixes to reported files.
    #[arg(long, conflicts_with = "stdin_filename")]
    fix: bool,
    /// Count reported diagnostics and include suppression reasons and states.
    #[arg(long)]
    statistics: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum CheckFormat {
    Text,
    Concise,
    Json,
    Sarif,
    Github,
}

#[derive(Args)]
pub struct IndexArgs {
    /// Print the current workspace index as JSON.
    #[arg(long, required = true)]
    dump: bool,
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long)]
    no_cache: bool,
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
    index: Option<WorkspaceIndex>,
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
            CheckFormat::Sarif => {
                render_sarif(&self.diagnostics).map_err(|error| error.to_string())
            }
            CheckFormat::Github => Ok(render_github(&self.diagnostics)),
        }
    }

    fn print_errors(&self, github: bool) {
        for error in &self.policy.errors {
            let message = format!("{}: {}", error.filename, error.message);
            if github {
                print_github_log(&message);
            } else {
                eprintln!("{message}");
            }
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
    let mut evaluation = evaluate(&cwd, &args, replacement, false)?;
    if args.fix && evaluation.policy.errors.is_empty() {
        // Rebuild the snapshot before applying cross-file decisions.
        let fresh = evaluate(&cwd, &args, None, false)?;
        if !fresh.policy.errors.is_empty()
            || fresh.sources != evaluation.sources
            || fresh.diagnostics != evaluation.diagnostics
        {
            evaluation.error(
                ".",
                "Workspace inputs changed during the check; rerun before applying fixes.",
            );
        } else {
            let errors = apply_safe_fixes(&fresh);
            evaluation = evaluate(&cwd, &args, None, false)?;
            evaluation.policy.errors.extend(errors);
        }
    }
    write_stdout(&render_evaluation(
        &evaluation,
        args.output_format,
        args.statistics,
    )?)?;
    if args.statistics && matches!(args.output_format, CheckFormat::Github) {
        print_github_log(&statistics_text(&evaluation));
    }
    evaluation.print_errors(matches!(args.output_format, CheckFormat::Github));
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
        fix: false,
        statistics: false,
    };
    let evaluation = evaluate(&current_dir()?, &args, None, true)?;
    let mut output =
        serde_json::to_string_pretty(&evaluation.policy).map_err(|error| error.to_string())?;
    output.push('\n');
    write_stdout(&output)?;
    evaluation.print_errors(false);
    Ok(if evaluation.policy.errors.is_empty() {
        0
    } else {
        2
    })
}

pub fn index(args: IndexArgs) -> Result<u8, String> {
    let options = CheckArgs {
        paths: Vec::new(),
        config: args.config,
        selection: SelectionArgs::default(),
        output_format: CheckFormat::Json,
        stdin_filename: None,
        exit_zero: false,
        no_cache: args.no_cache,
        fix: false,
        statistics: false,
    };
    let evaluation = evaluate(&current_dir()?, &options, None, true)?;
    let index = evaluation
        .index
        .as_ref()
        .ok_or("Workspace index was not built.")?;
    let result = serde_json::json!({"index":index.dump(),"errors":evaluation.policy.errors});
    write_stdout(&(serde_json::to_string_pretty(&result).map_err(|e| e.to_string())? + "\n"))?;
    evaluation.print_errors(false);
    Ok(if evaluation.policy.errors.is_empty() {
        0
    } else {
        2
    })
}

fn statistics(evaluation: &Evaluation) -> serde_json::Value {
    let mut counts = BTreeMap::<&str, usize>::new();
    for diagnostic in &evaluation.diagnostics {
        *counts.entry(&diagnostic.code).or_default() += 1;
    }
    let suppressions: Vec<_> =
        evaluation
            .policy
            .files
            .iter()
            .flat_map(|file| {
                file.suppressions.as_array().into_iter().flatten().map(
                    |record| serde_json::json!({"filename":file.filename,"declaration":record}),
                )
            })
            .collect();
    serde_json::json!({"rules":counts,"suppressions":suppressions})
}

fn statistics_text(evaluation: &Evaluation) -> String {
    let stats = statistics(evaluation);
    let mut text = String::from("Rule counts:\n");
    for (code, count) in stats["rules"].as_object().into_iter().flatten() {
        text.push_str(&format!("  {code}: {count}\n"));
    }
    text.push_str("Suppressions:\n");
    for file in &evaluation.policy.files {
        for record in file.suppressions.as_array().into_iter().flatten() {
            let reason = record["reason"]
                .as_str()
                .unwrap_or("")
                .replace(['\r', '\n'], " ");
            text.push_str(&format!(
                "  {}: {} -- {} ({})\n",
                file.filename, record["codes"], reason, record["states"]
            ));
        }
    }
    text
}

fn print_github_log(text: &str) {
    // The runner also parses stderr, including legacy commands embedded within
    // a line. Prefix every line and break that legacy delimiter in log content.
    for line in text.lines() {
        eprintln!(
            "seiso: {}",
            line.replace('\r', "\\r").replace("##[", "## [")
        );
    }
}

fn render_evaluation(
    evaluation: &Evaluation,
    format: CheckFormat,
    include_statistics: bool,
) -> Result<String, String> {
    if !include_statistics {
        return evaluation.render(format);
    }
    match format {
        CheckFormat::Json => serde_json::to_string_pretty(&serde_json::json!({"diagnostics":evaluation.diagnostics,"statistics":statistics(evaluation)}))
            .map(|text|text+"\n").map_err(|error|error.to_string()),
        CheckFormat::Sarif => {
            let mut sarif: serde_json::Value = serde_json::from_str(&evaluation.render(format)?).map_err(|error|error.to_string())?;
            sarif["runs"][0]["properties"] = serde_json::json!({"statistics":statistics(evaluation)});
            serde_json::to_string_pretty(&sarif).map(|text|text+"\n").map_err(|error|error.to_string())
        }
        CheckFormat::Github => evaluation.render(format),
        _ => Ok(evaluation.render(format)?+&statistics_text(evaluation)),
    }
}

fn apply_safe_fixes(evaluation: &Evaluation) -> Vec<InputError> {
    let mut errors = Vec::new();
    let Some(index) = &evaluation.index else {
        return errors;
    };
    for file in &index.files {
        let diagnostics = evaluation
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.filename == file.filename)
            .cloned()
            .collect::<Vec<_>>();
        let outcome = (|| -> Result<(), String> {
            let Some(updated) =
                seiso_rules::fixes::apply_fixes(&file.document.source, &diagnostics)?
            else {
                return Ok(());
            };
            let metadata = fs::symlink_metadata(&file.path).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.permissions().readonly()
            {
                return Err("Cannot apply fixes to a symlink, non-file, or read-only file.".into());
            }
            let mut temporary = tempfile::NamedTempFile::new_in(
                file.path.parent().ok_or("Missing parent directory.")?,
            )
            .map_err(|e| e.to_string())?;
            temporary
                .as_file()
                .set_permissions(metadata.permissions())
                .map_err(|e| e.to_string())?;
            temporary
                .write_all(updated.as_bytes())
                .map_err(|e| e.to_string())?;
            temporary.flush().map_err(|e| e.to_string())?;
            if fs::read(&file.path).map_err(|e| e.to_string())? != file.document.source.as_bytes() {
                return Err(
                    "Source changed before the fix could be written; rerun the check.".into(),
                );
            }
            temporary.persist(&file.path).map_err(|e| e.to_string())?;
            Ok(())
        })();
        if let Err(message) = outcome {
            errors.push(InputError {
                filename: file.filename.clone(),
                message,
            });
        }
    }
    errors
}

struct PendingDocument {
    path: PathBuf,
    filename: String,
    config: seiso_config::Config,
    configuration: String,
    source_override: Option<String>,
}

type LoadedDocument = Result<(String, Arc<seiso_md::Document>), String>;

fn load_documents(
    inputs: &[PendingDocument],
    cache: &seiso_cache::ParseCache,
) -> Vec<LoadedDocument> {
    let load = |input: &PendingDocument| {
        let source = input
            .source_override
            .as_ref()
            .map_or_else(
                || fs::read_to_string(&input.path),
                |source| Ok(source.clone()),
            )
            .map_err(|error| format!("Cannot read UTF-8 Markdown: {error}"))?;
        let document = cache
            .parse_shared(&source)
            .map_err(|error| format!("Cannot parse Markdown: {error}"))?;
        Ok((source, document))
    };
    let workers = std::thread::available_parallelism()
        .map_or(1, |count| count.get())
        .min(4);
    if workers == 1 || inputs.len() < 32 {
        return inputs.iter().map(load).collect();
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = inputs
            .chunks(inputs.len().div_ceil(workers))
            .map(|batch| {
                let load = &load;
                (
                    batch.len(),
                    scope.spawn(move || batch.iter().map(load).collect::<Vec<_>>()),
                )
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|(count, handle)| {
                handle.join().unwrap_or_else(|_| {
                    (0..count)
                        .map(|_| Err("Markdown worker did not finish; rerun the check.".to_owned()))
                        .collect()
                })
            })
            .collect()
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
    let overlay = if let Some(path) = &args.stdin_filename {
        let path = workspace_path(&root, &absolute(cwd, path))?;
        if !is_markdown(&path) {
            return Err("The stdin filename must have a .md or .markdown extension.".into());
        }
        if ignored_by_git(&root, &path)? {
            return Err(format!(
                "{} is excluded by .gitignore; choose an included Markdown path.",
                relative(&root, &path)
            ));
        }
        requested.insert(path.clone());
        Some(path)
    } else {
        for path in &args.paths {
            let path = absolute(cwd, path);
            let path = workspace_path(&root, &path)?;
            match path.try_exists() {
                Ok(true) => {
                    requested.insert(path);
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
        None
    };
    let walker = ignore::WalkBuilder::new(&root)
        .hidden(false)
        .follow_links(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .parents(false)
        .require_git(false)
        .filter_entry(|entry| entry.file_name() != ".git" && entry.file_name() != ".seiso_cache")
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
                if !entry.file_type().is_some_and(|kind| kind.is_file()) || !is_markdown(&path) {
                    continue;
                }
                inputs.insert(path, None);
            }
            Err(error) => {
                evaluation.error(".", format!("Cannot finish workspace discovery: {error}"))
            }
        }
    }
    if let Some(path) = overlay {
        inputs.insert(path, replacement);
    }
    // Include the workspace configuration even when no Markdown file is selected.
    record_configuration(&mut evaluation, &root, &workspace.config, &overrides);
    let cache = seiso_cache::ParseCache::new(root.join(".seiso_cache"), !args.no_cache);
    let mut indexed = Vec::new();
    let mut configurations = BTreeMap::new();
    let mut selected_files = BTreeSet::new();
    let mut pending = Vec::new();
    for (path, source_override) in inputs {
        let filename = relative(&root, &path);
        let directory = path.parent().unwrap_or(&root).to_path_buf();
        let resolved = configurations.entry(directory).or_insert_with(|| {
            workspace
                .config_for(&path)
                .map_err(|error| error.to_string())
        });
        let config = match resolved {
            Ok(config) => config.clone(),
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
        pending.push(PendingDocument {
            path,
            filename,
            config,
            configuration,
            source_override,
        });
    }
    let loaded = load_documents(&pending, &cache);
    for (input, loaded) in pending.into_iter().zip(loaded) {
        let PendingDocument {
            path,
            filename,
            config,
            configuration,
            ..
        } = input;
        let (source, document) = match loaded {
            Ok(loaded) => loaded,
            Err(error) => {
                evaluation.error(filename, error);
                continue;
            }
        };
        let kind = seiso_rules::resolve_kind(&document, config.kind_for(&path));
        let enabled_rules = config
            .enabled_rules(&path, kind.value.as_deref(), &overrides)
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|code| seiso_rules::IMPLEMENTED_RULES.contains(code))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if (args.paths.is_empty() && args.stdin_filename.is_none())
            || requested.iter().any(|selected| path.starts_with(selected))
        {
            selected_files.insert(filename.clone());
            evaluation.enabled_count += enabled_rules.len();
        }
        evaluation.sources.insert(filename.clone(), source);
        evaluation.policy.files.push(FilePolicy {
            filename: filename.clone(),
            configuration,
            kind: Some(kind.clone()),
            domain: config.domain_for(&path).map(str::to_owned),
            enabled_rules: enabled_rules.clone(),
            excluded: None,
            suppressions: serde_json::json!([]),
        });
        indexed.push(IndexedFile {
            filename,
            path: path.clone(),
            document,
            kind: kind.value,
            domain: config.domain_for(&path).unwrap_or("").to_owned(),
            enabled_rules,
            config,
        });
    }
    let index = WorkspaceIndex::new(root.clone(), indexed, evaluation.policy.errors.is_empty());
    let mut cross = seiso_rules::cross_file::check(&index);
    for (filename, error) in &cross.errors {
        evaluation.error(filename, error);
    }
    let mut cross_by_file = BTreeMap::<String, Vec<Diagnostic>>::new();
    for diagnostic in cross.diagnostics {
        cross_by_file
            .entry(diagnostic.filename.clone())
            .or_default()
            .push(diagnostic);
    }
    for file in &index.files {
        let selected = selected_files.contains(&file.filename);
        let mut raw = if selected {
            seiso_rules::check_raw(&seiso_rules::CheckContext {
                document: &file.document,
                filename: &file.filename,
                path: &file.path,
                workspace_root: &root,
                config: &file.config,
                overrides: &overrides,
            })
            .map_err(|error| error.to_string())?
        } else {
            seiso_rules::RawCheckResult {
                kind: seiso_rules::resolve_kind(&file.document, file.config.kind_for(&file.path)),
                enabled_rules: BTreeSet::new(),
                diagnostics: Vec::new(),
                errors: Vec::new(),
                incomplete_rules: seiso_rules::SINGLE_FILE_RULES
                    .iter()
                    .map(|code| (*code).to_owned())
                    .collect(),
            }
        };
        raw.enabled_rules = file.enabled_rules.iter().cloned().collect();
        raw.diagnostics
            .extend(cross_by_file.remove(&file.filename).unwrap_or_default());
        raw.incomplete_rules
            .extend(cross.incomplete.remove(&file.filename).unwrap_or_default());
        if !index.complete {
            raw.incomplete_rules.extend(
                seiso_rules::CROSS_FILE_RULES
                    .iter()
                    .map(|code| (*code).to_owned()),
            );
        }
        let result = seiso_rules::finish_check(&file.document, &file.filename, raw);
        for error in result.errors {
            evaluation.error(&file.filename, error);
        }
        evaluation
            .diagnostics
            .extend(result.diagnostics.into_iter().filter(|diagnostic| {
                selected_files.contains(&diagnostic.filename)
                    || (seiso_rules::CROSS_FILE_RULES.contains(&diagnostic.code.as_str())
                        && diagnostic.related.iter().any(|related| {
                            selected_files.contains(&related.filename)
                                || (diagnostic.code == "PTR002"
                                    && requested.iter().any(|selected| {
                                        root.join(&related.filename).starts_with(selected)
                                    }))
                        }))
            }));
        if let Some(policy) = evaluation
            .policy
            .files
            .iter_mut()
            .find(|policy| policy.filename == file.filename)
        {
            policy.suppressions =
                serde_json::to_value(result.suppressions).map_err(|error| error.to_string())?;
        }
    }
    evaluation.diagnostics = seiso_diagnostics::sorted_diagnostics(&evaluation.diagnostics);
    if !policy_mode {
        evaluation.policy.files.retain(|file| {
            selected_files.contains(&file.filename)
                || evaluation
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.filename == file.filename)
        });
    }
    evaluation.index = Some(index);
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
        no_cache: false,
        fix: false,
        statistics: false,
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
            evaluation.print_errors(false);
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
