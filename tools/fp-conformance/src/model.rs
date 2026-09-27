use std::collections::BTreeMap;

pub const VERSION: &str = "1.0.0";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Language {
    Rust,
    TypeScript,
    Dart,
}

impl Language {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => return "rust",
            Self::TypeScript => return "ts",
            Self::Dart => return "dart",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Severity {
    Error,
    Warn,
    Info,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => return "error",
            Self::Warn => return "warn",
            Self::Info => return "info",
        }
    }

    pub const fn rank(self) -> u8 {
        match self {
            Self::Error => return 0,
            Self::Warn => return 1,
            Self::Info => return 2,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub code: &'static str,
    pub lang: Option<Language>,
    pub severity: Severity,
    pub principle: &'static str,
    pub title: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding {
    pub code: &'static str,
    pub severity: Severity,
    pub lang: Language,
    pub path: String,
    pub line: usize,
    pub text: String,
    pub title: &'static str,
    pub principle: &'static str,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    pub files: usize,
    pub lines: usize,
    pub rust: usize,
    pub ts: usize,
    pub dart: usize,
}

impl Stats {
    pub fn record(&mut self, lang: Language, lines: usize) {
        self.files = self.files.saturating_add(1);
        self.lines = self.lines.saturating_add(lines);
        match lang {
            Language::Rust => {
                self.rust = self.rust.saturating_add(1);
            }
            Language::TypeScript => {
                self.ts = self.ts.saturating_add(1);
            }
            Language::Dart => {
                self.dart = self.dart.saturating_add(1);
            }
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScanResult {
    pub findings: Vec<Finding>,
    pub stats: Stats,
}

impl ScanResult {
    pub fn counts(&self) -> BTreeMap<String, usize> {
        let counts = self.findings.iter().fold(BTreeMap::new(), |mut acc, finding| {
            let entry = acc.entry(finding.code.to_owned()).or_insert(0_usize);
            *entry = entry.saturating_add(1);
            return acc;
        });
        return counts;
    }
}
