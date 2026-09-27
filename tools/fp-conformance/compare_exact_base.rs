use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug)]
enum CompareError {
    Usage(String),
    Io(std::io::Error),
    InvalidLine { path: PathBuf, line: usize, value: String },
    InvalidCount { path: PathBuf, line: usize, value: String },
}

impl fmt::Display for CompareError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => {
                return write!(formatter, "{message}");
            }
            Self::Io(error) => {
                return write!(formatter, "I/O error: {error}");
            }
            Self::InvalidLine { path, line, value } => {
                return write!(
                    formatter,
                    "{}:{line}: expected RULE<TAB>COUNT, got {value:?}",
                    path.display()
                );
            }
            Self::InvalidCount { path, line, value } => {
                return write!(
                    formatter,
                    "{}:{line}: invalid non-negative count {value:?}",
                    path.display()
                );
            }
        }
    }
}

impl std::error::Error for CompareError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => {
                return Some(error);
            }
            Self::Usage(_) | Self::InvalidLine { .. } | Self::InvalidCount { .. } => {
                return None;
            }
        }
    }
}

impl From<std::io::Error> for CompareError {
    fn from(error: std::io::Error) -> Self {
        return Self::Io(error);
    }
}

#[derive(Debug)]
struct Cli {
    base: PathBuf,
    head: PathBuf,
}

fn parse_args(args: Vec<OsString>) -> Result<Cli, CompareError> {
    match args.as_slice() {
        [base_flag, base, head_flag, head]
            if base_flag == "--base" && head_flag == "--head" =>
        {
            return Ok(Cli {
                base: PathBuf::from(base),
                head: PathBuf::from(head),
            });
        }
        _ => {
            return Err(CompareError::Usage(
                "usage: compare_exact_base --base <counts.tsv> --head <counts.tsv>".to_owned(),
            ));
        }
    }
}

fn read_counts(path: &Path) -> Result<BTreeMap<String, u64>, CompareError> {
    let content = fs::read_to_string(path)?;
    return content.lines().enumerate().try_fold(
        BTreeMap::new(),
        |mut counts, (offset, line)| {
            if line.trim().is_empty() {
                return Ok(counts);
            }
            let (code, raw_count) = line
                .split_once('	')
                .ok_or_else(|| CompareError::InvalidLine {
                    path: path.to_path_buf(),
                    line: offset + 1,
                    value: line.to_owned(),
                })?;
            if code.is_empty() || code.bytes().any(|byte| !byte.is_ascii_alphanumeric()) {
                return Err(CompareError::InvalidLine {
                    path: path.to_path_buf(),
                    line: offset + 1,
                    value: line.to_owned(),
                });
            }
            let count = raw_count
                .parse::<u64>()
                .map_err(|_| CompareError::InvalidCount {
                    path: path.to_path_buf(),
                    line: offset + 1,
                    value: raw_count.to_owned(),
                })?;
            counts.insert(code.to_owned(), count);
            return Ok(counts);
        },
    );
}

fn compare(base: &BTreeMap<String, u64>, head: &BTreeMap<String, u64>) -> bool {
    let codes = base.keys().chain(head.keys()).collect::<std::collections::BTreeSet<_>>();
    let improvements = codes
        .iter()
        .filter_map(|code| {
            let before = base.get(*code).copied().unwrap_or(0);
            let after = head.get(*code).copied().unwrap_or(0);
            if after < before {
                return Some((code, before, after));
            }
            return None;
        })
        .collect::<Vec<_>>();
    let regressions = codes
        .iter()
        .filter_map(|code| {
            let before = base.get(*code).copied().unwrap_or(0);
            let after = head.get(*code).copied().unwrap_or(0);
            if after > before {
                return Some((code, before, after));
            }
            return None;
        })
        .collect::<Vec<_>>();

    if !improvements.is_empty() {
        println!("fp-conformance improvements:");
        for (code, before, after) in improvements {
            println!("  {code}: {before} -> {after}");
        }
    }

    if regressions.is_empty() {
        println!("fp-conformance: pass; no rule count increased from exact base");
        return true;
    }

    eprintln!("fp-conformance: regression against exact base");
    for (code, before, after) in regressions {
        eprintln!("  {code}: {before} -> {after}");
    }
    return false;
}

fn run() -> Result<bool, CompareError> {
    let cli = parse_args(env::args_os().skip(1).collect::<Vec<_>>())?;
    let base = read_counts(&cli.base)?;
    let head = read_counts(&cli.head)?;
    return Ok(compare(&base, &head));
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => {
            return ExitCode::SUCCESS;
        }
        Ok(false) => {
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("compare_exact_base: {error}");
            return ExitCode::from(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, u64)]) -> BTreeMap<String, u64> {
        return entries
            .iter()
            .map(|(code, count)| ((*code).to_owned(), *count))
            .collect();
    }

    #[test]
    fn identical_counts_pass() {
        let base = map(&[("RS001", 4), ("RS003", 2)]);
        assert!(compare(&base, &base));
    }

    #[test]
    fn improvements_pass() {
        assert!(compare(
            &map(&[("RS001", 4), ("RS003", 2)]),
            &map(&[("RS001", 3), ("RS003", 2)])
        ));
    }

    #[test]
    fn new_rule_is_a_regression_from_zero() {
        assert!(!compare(
            &map(&[("RS001", 4)]),
            &map(&[("RS001", 4), ("RS002", 1)])
        ));
    }

    #[test]
    fn increased_existing_rule_fails() {
        assert!(!compare(
            &map(&[("RS001", 4)]),
            &map(&[("RS001", 5)])
        ));
    }
}
