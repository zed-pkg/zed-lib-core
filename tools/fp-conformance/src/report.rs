use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use crate::model::{ScanResult, Severity, VERSION};
use crate::rules::rule;

pub fn render(result: &ScanResult, limit: usize) -> String {
    let error = result
        .findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .count();
    let warn = result
        .findings
        .iter()
        .filter(|finding| finding.severity == Severity::Warn)
        .count();
    let info = result
        .findings
        .iter()
        .filter(|finding| finding.severity == Severity::Info)
        .count();
    let counts = result.counts();
    let mut output = vec![
        format!("fp-conformance {VERSION}"),
        format!(
            "  scanned {} files ({} rust, {} ts, {} dart), {} lines",
            result.stats.files,
            result.stats.rust,
            result.stats.ts,
            result.stats.dart,
            result.stats.lines
        ),
        format!("  findings: {error} error, {warn} warn, {info} info"),
        String::new(),
    ];
    if !counts.is_empty() {
        output.push("by rule:".to_owned());
        let mut ordered = counts.iter().collect::<Vec<_>>();
        ordered.sort_by(|left, right| {
            return right
                .1
                .cmp(left.1)
                .then_with(|| left.0.cmp(right.0));
        });
        for (code, count) in ordered {
            if let Some(metadata) = rule(code) {
                output.push(format!(
                    "  {code}  {count:6}  [{}] {}",
                    metadata.severity.as_str(),
                    metadata.title
                ));
            }
        }
        output.push(String::new());
    }

    let shown = result.findings.iter().take(limit).collect::<Vec<_>>();
    if !shown.is_empty() {
        output.push(format!("first {} findings:", shown.len()));
        for finding in shown {
            output.push(format!(
                "  {}:{}: [{}] {} {}",
                finding.path,
                finding.line,
                finding.severity.as_str(),
                finding.code,
                finding.title
            ));
            if !finding.text.is_empty() {
                output.push(format!("      {}", finding.text));
            }
        }
    }
    if result.findings.len() > limit {
        output.push(format!(
            "  ... and {} more",
            result.findings.len().saturating_sub(limit)
        ));
    }
    return output.join("\n");
}

pub fn write_json(path: &Path, result: &ScanResult) -> io::Result<()> {
    let counts = result.counts();
    let mut output = String::new();
    output.push_str("{\n");
    output.push_str(&format!(" \"version\": \"{}\",\n", escape_json(VERSION)));
    output.push_str(&format!(
        " \"stats\": {{\"files\": {}, \"lines\": {}, \"rust\": {}, \"ts\": {}, \"dart\": {}}},\n",
        result.stats.files,
        result.stats.lines,
        result.stats.rust,
        result.stats.ts,
        result.stats.dart
    ));
    write_count_object(&mut output, "counts", &counts, true);
    output.push_str(" \"findings\": [\n");
    for (index, finding) in result.findings.iter().enumerate() {
        output.push_str(&format!(
            "  {{\"code\":\"{}\",\"severity\":\"{}\",\"lang\":\"{}\",\"path\":\"{}\",\"line\":{},\"text\":\"{}\",\"title\":\"{}\",\"principle\":\"{}\"}}{}\n",
            escape_json(finding.code),
            escape_json(finding.severity.as_str()),
            escape_json(finding.lang.as_str()),
            escape_json(&finding.path),
            finding.line,
            escape_json(&finding.text),
            escape_json(finding.title),
            escape_json(finding.principle),
            if index.saturating_add(1) == result.findings.len() {
                ""
            } else {
                ","
            }
        ));
    }
    output.push_str(" ]\n}\n");
    return fs::write(path, output);
}

pub fn write_budget(path: &Path, result: &ScanResult) -> io::Result<()> {
    let counts = result.counts();
    let mut output = String::new();
    output.push_str("{\n");
    write_count_object(&mut output, "budget", &counts, true);
    output.push_str(&format!(
        " \"stats\": {{\"dart\": {}, \"files\": {}, \"lines\": {}, \"rust\": {}, \"ts\": {}}},\n",
        result.stats.dart,
        result.stats.files,
        result.stats.lines,
        result.stats.rust,
        result.stats.ts
    ));
    output.push_str(&format!(
        " \"version\": \"{}\"\n",
        escape_json(VERSION)
    ));
    output.push_str("}\n");
    return fs::write(path, output);
}

fn write_count_object(
    output: &mut String,
    key: &str,
    counts: &BTreeMap<String, usize>,
    comma_after: bool,
) {
    output.push_str(&format!(" \"{key}\": {{\n"));
    for (index, (code, count)) in counts.iter().enumerate() {
        let comma = if index.saturating_add(1) == counts.len() {
            ""
        } else {
            ","
        };
        output.push_str(&format!(
            "  \"{}\": {}{}\n",
            escape_json(code),
            count,
            comma
        ));
    }
    output.push_str(if comma_after { " },\n" } else { " }\n" });
}

pub fn read_count_object(path: &Path, object: &str) -> io::Result<BTreeMap<String, usize>> {
    let content = fs::read_to_string(path)?;
    return parse_count_object(&content, object).ok_or_else(|| {
        return io::Error::new(
            io::ErrorKind::InvalidData,
            format!("missing {object} object"),
        );
    });
}

pub fn parse_count_object(content: &str, object: &str) -> Option<BTreeMap<String, usize>> {
    let needle = format!("\"{object}\"");
    let start = content.find(&needle)?;
    let object_start = content[start..]
        .find('{')?
        .saturating_add(start)
        .saturating_add(1);
    let object_end = find_matching_brace(content, object_start.saturating_sub(1))?;
    let body = &content[object_start..object_end];
    let counts = body
        .split(',')
        .filter_map(|entry| {
            let (key, value) = entry.split_once(':')?;
            let code = key.trim().trim_matches('"');
            let count = value.trim().parse::<usize>().ok()?;
            return Some((code.to_owned(), count));
        })
        .collect::<BTreeMap<_, _>>();
    return Some(counts);
}

fn find_matching_brace(content: &str, open: usize) -> Option<usize> {
    let mut depth = 0_isize;
    let mut in_string = false;
    let mut escaped = false;
    for (relative, ch) in content[open..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            continue;
        }
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                return Some(open.saturating_add(relative));
            }
        }
    }
    return None;
}

pub fn regressions(
    base: &BTreeMap<String, usize>,
    head: &BTreeMap<String, usize>,
) -> Vec<(String, usize, usize)> {
    let mut codes = base
        .keys()
        .chain(head.keys())
        .cloned()
        .collect::<Vec<_>>();
    codes.sort();
    codes.dedup();
    return codes
        .into_iter()
        .filter_map(|code| {
            let before = base.get(&code).copied().unwrap_or(0);
            let after = head.get(&code).copied().unwrap_or(0);
            if after > before {
                return Some((code, before, after));
            }
            return None;
        })
        .collect();
}

pub fn improvements(
    base: &BTreeMap<String, usize>,
    head: &BTreeMap<String, usize>,
) -> Vec<(String, usize, usize)> {
    let mut codes = base
        .keys()
        .chain(head.keys())
        .cloned()
        .collect::<Vec<_>>();
    codes.sort();
    codes.dedup();
    return codes
        .into_iter()
        .filter_map(|code| {
            let before = base.get(&code).copied().unwrap_or(0);
            let after = head.get(&code).copied().unwrap_or(0);
            if after < before {
                return Some((code, before, after));
            }
            return None;
        })
        .collect();
}

fn escape_json(value: &str) -> String {
    return value
        .chars()
        .flat_map(|ch| {
            return match ch {
                '"' => "\\\"".chars().collect::<Vec<_>>(),
                '\\' => "\\\\".chars().collect::<Vec<_>>(),
                '\n' => "\\n".chars().collect::<Vec<_>>(),
                '\r' => "\\r".chars().collect::<Vec<_>>(),
                '\t' => "\\t".chars().collect::<Vec<_>>(),
                value if value.is_control() => format!("\\u{:04x}", u32::from(value))
                    .chars()
                    .collect::<Vec<_>>(),
                value => vec![value],
            };
        })
        .collect();
}
