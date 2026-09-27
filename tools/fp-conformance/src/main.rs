use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use fp_conformance::model::Severity;
use fp_conformance::report::{
    improvements, read_count_object, regressions, render, write_budget, write_json,
};
use fp_conformance::rules::rule;
use fp_conformance::scanner::analyse;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailOn {
    Never,
    Error,
    Warn,
}

#[derive(Debug)]
struct Options {
    paths: Vec<PathBuf>,
    json: Option<PathBuf>,
    budget: Option<PathBuf>,
    write_budget: Option<PathBuf>,
    compare_json: Option<PathBuf>,
    fail_on: FailOn,
    limit: usize,
    quiet: bool,
}

fn parse_args() -> Result<Options, String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut paths = Vec::new();
    let mut json = None;
    let mut budget = None;
    let mut write_budget = None;
    let mut compare_json = None;
    let mut fail_on = FailOn::Never;
    let mut limit = 40_usize;
    let mut quiet = false;
    let mut index = 0_usize;

    while index < args.len() {
        let value = &args[index];
        if matches!(
            value.as_str(),
            "--json" | "--budget" | "--write-budget" | "--compare-json" | "--fail-on" | "--limit"
        ) {
            let next = args
                .get(index.saturating_add(1))
                .ok_or_else(|| format!("missing value for {value}"))?;
            if value == "--json" {
                json = Some(PathBuf::from(next));
            } else if value == "--budget" {
                budget = Some(PathBuf::from(next));
            } else if value == "--write-budget" {
                write_budget = Some(PathBuf::from(next));
            } else if value == "--compare-json" {
                compare_json = Some(PathBuf::from(next));
            } else if value == "--fail-on" {
                fail_on = match next.as_str() {
                    "never" => FailOn::Never,
                    "error" => FailOn::Error,
                    "warn" => FailOn::Warn,
                    _ => return Err(format!("invalid --fail-on value: {next}")),
                };
            } else {
                limit = next
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --limit: {next}"))?;
            }
            index = index.saturating_add(2);
            continue;
        }

        if value == "--quiet" {
            quiet = true;
            index = index.saturating_add(1);
            continue;
        }
        if value.starts_with('-') {
            return Err(format!("unknown option: {value}"));
        }
        paths.push(PathBuf::from(value));
        index = index.saturating_add(1);
    }

    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    return Ok(Options {
        paths,
        json,
        budget,
        write_budget,
        compare_json,
        fail_on,
        limit,
        quiet,
    });
}

fn run() -> Result<ExitCode, String> {
    let options = parse_args()?;
    let result = analyse(&options.paths);
    let counts = result.counts();

    if let Some(path) = options.write_budget.as_deref() {
        write_budget(path, &result).map_err(|error| error.to_string())?;
        if !options.quiet {
            println!(
                "wrote budget for {} rules to {}",
                counts.len(),
                path.display()
            );
        }
        return Ok(ExitCode::SUCCESS);
    }

    if let Some(path) = options.json.as_deref() {
        write_json(path, &result).map_err(|error| error.to_string())?;
    }
    if !options.quiet {
        println!("{}", render(&result, options.limit));
    }

    let mut failed = false;
    if let Some(path) = options.budget.as_deref() {
        let baseline =
            read_count_object(path, "budget").map_err(|error| error.to_string())?;
        let deltas = regressions(&baseline, &counts);
        if !deltas.is_empty() {
            println!("\nfp-conformance: regression against budget");
            for (code, before, after) in deltas {
                let title = rule(&code)
                    .map(|metadata| metadata.title)
                    .unwrap_or("unknown rule");
                println!("  {code} {title}: {before} -> {after}");
            }
            println!(
                "\nFix the new occurrences; do not expand the baseline to hide them."
            );
            failed = true;
        }
    }

    if let Some(path) = options.compare_json.as_deref() {
        let baseline =
            read_count_object(path, "counts").map_err(|error| error.to_string())?;
        let better = improvements(&baseline, &counts);
        if !better.is_empty() {
            println!("\nfp-conformance improvements from exact base:");
            for (code, before, after) in better {
                println!("  {code}: {before} -> {after}");
            }
        }

        let deltas = regressions(&baseline, &counts);
        if deltas.is_empty() {
            println!(
                "\nfp-conformance: pass; no rule count increased from exact base"
            );
        } else {
            println!("\nfp-conformance: regression against exact base");
            for (code, before, after) in deltas {
                println!("  {code}: {before} -> {after}");
            }
            failed = true;
        }
    }

    match options.fail_on {
        FailOn::Never => {}
        FailOn::Error => {
            if result
                .findings
                .iter()
                .any(|finding| finding.severity == Severity::Error)
            {
                failed = true;
            }
        }
        FailOn::Warn => {
            if !result.findings.is_empty() {
                failed = true;
            }
        }
    }

    if failed {
        return Ok(ExitCode::FAILURE);
    }
    return Ok(ExitCode::SUCCESS);
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => return code,
        Err(error) => {
            eprintln!("fp-conformance: {error}");
            return ExitCode::from(2);
        }
    }
}
