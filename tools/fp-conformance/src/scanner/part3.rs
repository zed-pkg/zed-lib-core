fn findings_for_context(context: &FileContext) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (offset, line) in context.lines.iter().enumerate() {
        let line_number = offset.saturating_add(1);
        if !context.code_lines.contains(&line_number) {
            continue;
        }
        match context.lang {
            Language::Rust => scan_rust_line(context, line_number, line, &mut findings),
            Language::TypeScript => scan_ts_line(context, line_number, line, &mut findings),
            Language::Dart => scan_dart_line(context, line_number, line, &mut findings),
        }
    }
    scan_cross_language(context, &mut findings);
    return findings;
}

fn push_finding(context: &FileContext, findings: &mut Vec<Finding>, code: &'static str, line: usize, text: String) {
    let Some(metadata) = rule(code) else {
        return;
    };
    findings.push(Finding {
        code,
        severity: metadata.severity,
        lang: context.lang,
        path: context.rel.clone(),
        line,
        text,
        title: metadata.title,
        principle: metadata.principle,
    });
}

fn scan_rust_line(context: &FileContext, number: usize, line: &str, findings: &mut Vec<Finding>) {
    if !context.is_stateful && contains_word_pair(line, "let", "mut") {
        push_finding(context, findings, "RS001", number, excerpt(line));
    }
    if !context.is_effect_boundary {
        if contains_word_pair(line, "static", "mut") || is_global_lock(line) {
            push_finding(context, findings, "RS002", number, excerpt(line));
        }
    }
    if contains_any(line, &[".unwrap()", ".expect(", "panic!(", "unreachable!(", "todo!(", ".unwrap_unchecked("]) {
        push_finding(context, findings, "RS003", number, excerpt(line));
    }
    if wildcard_arm(line) {
        push_finding(context, findings, "RS004", number, excerpt(line));
    }
    if untyped_error(line) {
        push_finding(context, findings, "RS005", number, excerpt(line));
    }
    if !context.is_stateful && !context.is_effect_boundary && contains_interior_mutability(line) {
        push_finding(context, findings, "RS006", number, excerpt(line));
    }
    if !context.is_effect_boundary && contains_any(line, &["println!(", "eprintln!(", "print!(", "eprint!(", "dbg!("]) {
        push_finding(context, findings, "RS007", number, excerpt(line));
    }
    if !context.is_stateful && line.contains("fn ") && line.contains("&mut self") && line.contains('{') && !line.contains("->") {
        push_finding(context, findings, "RS008", number, excerpt(line));
    }
    if contains_word_followed_by(line, "unsafe", '{') {
        push_finding(context, findings, "RS009", number, excerpt(line));
    }
}

fn scan_ts_line(context: &FileContext, number: usize, line: &str, findings: &mut Vec<Finding>) {
    if declaration(line, "var") {
        push_finding(context, findings, "TS001", number, excerpt(line));
    }
    if !context.is_stateful && declaration(line, "let") && !top_level_ts_mutable(line) {
        push_finding(context, findings, "TS002", number, excerpt(line));
    }
    if !context.is_effect_boundary && top_level_ts_mutable(line) {
        push_finding(context, findings, "TS003", number, excerpt(line));
    }
    if method_call(line, MUTATOR_TS) || delete_mutation(line) {
        push_finding(context, findings, "TS004", number, excerpt(line));
    }
    if ts_any(line) {
        push_finding(context, findings, "TS005", number, excerpt(line));
    }
    if contains_word(line, "throw") {
        push_finding(context, findings, "TS006", number, excerpt(line));
    }
    if ts_react(line) {
        push_finding(context, findings, "TS007", number, excerpt(line));
    }
    if !context.is_effect_boundary && contains_any(line, &["console.log(", "console.debug(", "console.info(", "console.warn(", "console.error("]) {
        push_finding(context, findings, "TS008", number, excerpt(line));
    }
    if !context.is_effect_boundary && contains_any(line, &["Date.now", "Math.random", "new Date", "process.env", "crypto.randomUUID"]) {
        push_finding(context, findings, "TS009", number, excerpt(line));
    }
    if nonnull_assertion(line) {
        push_finding(context, findings, "TS010", number, excerpt(line));
    }
}

fn scan_dart_line(context: &FileContext, number: usize, line: &str, findings: &mut Vec<Finding>) {
    if !context.is_stateful && declaration(line, "var") {
        push_finding(context, findings, "DA001", number, excerpt(line));
    }
    if !context.is_effect_boundary && dart_top_mutable(line) {
        push_finding(context, findings, "DA002", number, excerpt(line));
    }
    if !context.is_stateful && dart_nonfinal_field(line) {
        push_finding(context, findings, "DA003", number, excerpt(line));
    }
    if dart_late(line) {
        push_finding(context, findings, "DA004", number, excerpt(line));
    }
    if contains_word(line, "throw") {
        push_finding(context, findings, "DA005", number, excerpt(line));
    }
    if !context.is_effect_boundary && contains_any(line, &["print(", "debugPrint("]) {
        push_finding(context, findings, "DA006", number, excerpt(line));
    }
    if nonnull_assertion(line) {
        push_finding(context, findings, "DA007", number, excerpt(line));
    }
    if !context.is_stateful && method_call(line, MUTATOR_DART) {
        push_finding(context, findings, "DA008", number, excerpt(line));
    }
    if line.trim_start().starts_with("default") && line.trim_start()["default".len()..].trim_start().starts_with(':') {
        push_finding(context, findings, "DA009", number, excerpt(line));
    }
}

fn scan_cross_language(context: &FileContext, findings: &mut Vec<Finding>) {
    let limit = if matches!(context.basename(), "main.rs" | "lib.rs" | "main.ts" | "index.ts" | "main.dart") { 250 } else { 600 };
    if context.nlines() > limit {
        push_finding(
            context,
            findings,
            "XX001",
            1,
            format!("{} is {} lines (limit {limit})", context.basename(), context.nlines()),
        );
    }
    for (line, name, length) in long_functions(context) {
        if length > 60 {
            push_finding(context, findings, "XX002", line, format!("`{name}` spans {length} lines"));
        }
    }
}
