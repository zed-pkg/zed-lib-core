use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::model::{Finding, Language, ScanResult, Stats};
use crate::rules::rule;

const SKIP_DIRS: &[&str] = &[
    ".git", "node_modules", "target", "build", "dist", "out", ".next", ".dart_tool",
    "vendor", "coverage", "Pods", ".venv", "venv", "__pycache__", ".idea", ".vscode",
    "generated", "gen", ".gradle", "Carthage",
];
const EXEMPT_SEGMENTS: &[&str] = &[
    "test", "tests", "spec", "specs", "example", "examples", "bench", "benches",
    "__tests__", "e2e", "fixture", "fixtures", "mock", "mocks", "migration", "migrations",
];
const EFFECT_TOKENS: &[&str] = &[
    "main", "bin", "cmd", "effect", "effects", "io", "adapter", "adapters", "infra",
    "infrastructure", "runtime", "transport", "server", "daemon", "sidecar", "wire", "db",
    "store", "repository", "repositories", "handler", "handlers", "route", "routes",
    "middleware", "telemetry", "otel", "logging", "log",
];
const STATEFUL_TOKENS: &[&str] = &[
    "ws", "websocket", "socket", "conn", "connection", "session", "pool", "cache", "buffer",
    "stream", "actor", "supervisor", "state_machine", "statemachine", "fsm",
];
const MUTATOR_TS: &[&str] = &["push", "pop", "shift", "unshift", "splice", "sort", "reverse", "fill", "copyWithin"];
const MUTATOR_DART: &[&str] = &["add", "addAll", "remove", "removeAt", "removeWhere", "clear", "insert", "sort", "shuffle"];

#[derive(Clone, Debug)]
struct FileContext {
    rel: String,
    lang: Language,
    lines: Vec<String>,
    code_lines: BTreeSet<usize>,
    is_effect_boundary: bool,
    is_stateful: bool,
}

impl FileContext {
    fn nlines(&self) -> usize {
        return self.lines.len();
    }

    fn basename(&self) -> &str {
        return self.rel.rsplit('/').next().unwrap_or(self.rel.as_str());
    }
}

pub fn analyse(roots: &[PathBuf]) -> ScanResult {
    let files = roots
        .iter()
        .flat_map(|root| source_files(root))
        .collect::<Vec<_>>();
    let mut findings = Vec::new();
    let mut stats = Stats::default();

    for source in files {
        let Some(context) = build_context(&source.path, &source.rel, source.lang) else {
            continue;
        };
        stats.record(context.lang, context.nlines());
        findings.extend(findings_for_context(&context));
    }

    findings.sort_by(|left, right| {
        return left
            .severity
            .rank()
            .cmp(&right.severity.rank())
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.line.cmp(&right.line));
    });
    return ScanResult { findings, stats };
}

pub fn analyse_text(rel: &str, lang: Language, source: &str) -> ScanResult {
    let (lines, code_lines) = strip_noise(source.split('\n').map(str::to_owned).collect());
    let context = FileContext {
        rel: rel.to_owned(),
        lang,
        lines,
        code_lines,
        is_effect_boundary: contains_path_token(rel, EFFECT_TOKENS, false),
        is_stateful: contains_path_token(rel, STATEFUL_TOKENS, true),
    };
    let stats = {
        let mut value = Stats::default();
        value.record(lang, context.nlines());
        value
    };
    return ScanResult {
        findings: findings_for_context(&context),
        stats,
    };
}

#[derive(Clone, Debug)]
struct SourceFile {
    path: PathBuf,
    rel: String,
    lang: Language,
}

fn source_files(root: &Path) -> Vec<SourceFile> {
    let absolute = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if absolute.is_file() {
        return language_for_path(&absolute)
            .map(|lang| SourceFile {
                rel: absolute
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned(),
                path: absolute,
                lang,
            })
            .into_iter()
            .collect();
    }
    if !absolute.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    walk_dir(&absolute, &absolute, &mut files);
    files.sort_by(|left, right| left.rel.cmp(&right.rel));
    return files;
}

fn walk_dir(root: &Path, dir: &Path, files: &mut Vec<SourceFile>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk_dir(root, &path, files);
            continue;
        }
        let Some(lang) = language_for_path(&path) else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let rel = relative.to_string_lossy().replace('\\', "/");
        if exempt_path(&rel) || rel.starts_with("tools/fp-conformance/") {
            continue;
        }
        files.push(SourceFile { path, rel, lang });
    }
}

fn language_for_path(path: &Path) -> Option<Language> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "rs" => return Some(Language::Rust),
        "ts" | "tsx" | "mts" | "cts" => return Some(Language::TypeScript),
        "dart" => return Some(Language::Dart),
        _ => return None,
    }
}

fn exempt_path(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    let segments = lower.split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| EXEMPT_SEGMENTS.contains(segment)) {
        return true;
    }
    let basename = segments.last().copied().unwrap_or_default();
    if basename == "build.rs" {
        return true;
    }
    let parts = basename.split('.').collect::<Vec<_>>();
    if parts.len() >= 3 {
        let marker = parts[parts.len() - 2];
        let ext = parts[parts.len() - 1];
        if ext.chars().all(|ch| ch.is_ascii_lowercase())
            && matches!(marker, "g" | "freezed" | "pb" | "generated" | "test" | "spec")
        {
            return true;
        }
    }
    return false;
}

fn build_context(path: &Path, rel: &str, lang: Language) -> Option<FileContext> {
    let content = fs::read_to_string(path).ok()?;
    let raw = content.split('\n').map(str::to_owned).collect::<Vec<_>>();
    if raw.len() > 20_000 {
        return None;
    }
    let (lines, code_lines) = strip_noise(raw);
    return Some(FileContext {
        rel: rel.to_owned(),
        lang,
        lines,
        code_lines,
        is_effect_boundary: contains_path_token(rel, EFFECT_TOKENS, false),
        is_stateful: contains_path_token(rel, STATEFUL_TOKENS, true),
    });
}

fn contains_path_token(rel: &str, tokens: &[&str], underscore_boundary: bool) -> bool {
    let lower = rel.to_ascii_lowercase();
    return tokens.iter().any(|token| {
        return lower.match_indices(token).any(|(index, _)| {
            let before = lower[..index].chars().next_back();
            let after_index = index.saturating_add(token.len());
            let after = lower[after_index..].chars().next();
            let before_ok = before.is_none() || before == Some('/');
            let after_ok = after.is_none()
                || matches!(after, Some('/') | Some('.'))
                || (underscore_boundary && after == Some('_'));
            return before_ok && after_ok;
        });
    });
}
