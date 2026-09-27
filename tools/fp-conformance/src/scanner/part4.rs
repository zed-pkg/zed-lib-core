fn long_functions(context: &FileContext) -> Vec<(usize, String, usize)> {
    let mut result = Vec::new();
    let mut open: Option<(usize, String, isize)> = None;
    for (offset, line) in context.lines.iter().enumerate() {
        let number = offset.saturating_add(1);
        if let Some((start, name, depth)) = open.take() {
            let next_depth = depth + brace_delta(line);
            if next_depth <= 0 {
                result.push((start, name, number.saturating_sub(start)));
            } else {
                open = Some((start, name, next_depth));
            }
            continue;
        }
        let Some(name) = function_start(context.lang, line) else {
            continue;
        };
        if !line.contains('{') {
            continue;
        }
        let depth = brace_delta(line);
        if depth > 0 {
            open = Some((number, name, depth));
        }
    }
    return result;
}

fn function_start(lang: Language, line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    match lang {
        Language::Rust => return rust_function_name(trimmed),
        Language::TypeScript => return ts_function_name(trimmed),
        Language::Dart => return dart_function_name(trimmed),
    }
}

fn rust_function_name(line: &str) -> Option<String> {
    let mut rest = line;
    for prefix in ["pub ", "async ", "unsafe "] {
        if let Some(value) = rest.strip_prefix(prefix) {
            rest = value;
        }
    }
    let rest = rest.strip_prefix("fn ")?;
    return identifier_prefix(rest);
}

fn ts_function_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix("export ").unwrap_or(line);
    let rest = rest.strip_prefix("async ").unwrap_or(rest);
    if let Some(value) = rest.strip_prefix("function ") {
        return identifier_prefix(value);
    }
    let rest = rest.strip_prefix("const ")?;
    let name = identifier_prefix(rest)?;
    let tail = rest.strip_prefix(name.as_str())?.trim_start();
    let tail = tail.strip_prefix('=')?.trim_start();
    let tail = tail.strip_prefix("async ").unwrap_or(tail);
    if tail.starts_with('(') {
        return Some(name);
    }
    return None;
}

fn dart_function_name(line: &str) -> Option<String> {
    if !line.contains('(') || !line.contains(')') || !line.contains('{') || line.trim_end().ends_with(';') {
        return None;
    }
    let before_paren = line.split('(').next()?.trim_end();
    let name = before_paren.split_whitespace().next_back()?;
    if matches!(name, "if" | "for" | "while" | "switch" | "catch") {
        return None;
    }
    if name.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_') {
        return Some(name.to_owned());
    }
    return None;
}

fn brace_delta(line: &str) -> isize {
    let opens = line.bytes().filter(|byte| *byte == b'{').count() as isize;
    let closes = line.bytes().filter(|byte| *byte == b'}').count() as isize;
    return opens.saturating_sub(closes);
}

fn identifier_prefix(value: &str) -> Option<String> {
    let ident = value
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>();
    if ident.is_empty() {
        return None;
    }
    return Some(ident);
}

fn excerpt(line: &str) -> String {
    return line.trim().chars().take(160).collect();
}

fn contains_any(line: &str, needles: &[&str]) -> bool {
    return needles.iter().any(|needle| line.contains(needle));
}

fn contains_word(line: &str, word: &str) -> bool {
    return line.match_indices(word).any(|(index, _)| word_boundaries(line, index, word.len()));
}

fn contains_word_pair(line: &str, first: &str, second: &str) -> bool {
    let mut tokens = line.split_whitespace();
    let mut previous = tokens.next();
    for token in tokens {
        if previous == Some(first) && token == second {
            return true;
        }
        previous = Some(token);
    }
    return false;
}

fn contains_word_followed_by(line: &str, word: &str, next: char) -> bool {
    return line.match_indices(word).any(|(index, _)| {
        if !word_boundaries(line, index, word.len()) {
            return false;
        }
        return line[index + word.len()..].trim_start().starts_with(next);
    });
}
