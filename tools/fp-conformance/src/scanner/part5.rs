fn word_boundaries(line: &str, index: usize, len: usize) -> bool {
    let before = line[..index].chars().next_back();
    let after = line[index.saturating_add(len)..].chars().next();
    let is_word = |ch: char| ch.is_ascii_alphanumeric() || ch == '_';
    return before.is_none_or(|ch| !is_word(ch) && ch != '.') && after.is_none_or(|ch| !is_word(ch));
}

fn declaration(line: &str, keyword: &str) -> bool {
    return line.match_indices(keyword).any(|(index, _)| {
        if !word_boundaries(line, index, keyword.len()) {
            return false;
        }
        let tail = &line[index + keyword.len()..];
        let Some(first) = tail.chars().next() else {
            return false;
        };
        if !first.is_whitespace() {
            return false;
        }
        let next = tail.trim_start().chars().next();
        return next.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '{' || ch == '[');
    });
}

fn top_level_ts_mutable(line: &str) -> bool {
    if line.chars().next().is_some_and(char::is_whitespace) {
        return false;
    }
    let rest = line.strip_prefix("export ").unwrap_or(line);
    return declaration(rest, "let") || declaration(rest, "var");
}

fn method_call(line: &str, methods: &[&str]) -> bool {
    return methods.iter().any(|method| line.contains(&format!(".{method}(")));
}

fn delete_mutation(line: &str) -> bool {
    let Some(index) = line.find("delete ") else {
        return false;
    };
    if !word_boundaries(line, index, "delete".len()) {
        return false;
    }
    let tail = line[index + "delete ".len()..].trim_start();
    let Some(identifier) = identifier_prefix(tail) else {
        return false;
    };
    let remainder = &tail[identifier.len()..];
    return remainder.starts_with('.') || remainder.starts_with('[');
}

fn ts_any(line: &str) -> bool {
    let compact = line.replace(' ', "");
    return compact.contains(":any") || compact.contains("<any>") || compact.contains("asany");
}

fn ts_react(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let has_loader = contains_word(&lower, "import") || contains_word(&lower, "require");
    return has_loader && contains_any(&lower, &["react", "react-dom", "preact"]);
}

fn nonnull_assertion(line: &str) -> bool {
    let chars = line.char_indices().collect::<Vec<_>>();
    return chars.windows(2).any(|window| {
        let (index, ch) = window[0];
        if ch != '!' {
            return false;
        }
        let before = line[..index].chars().next_back();
        if !before.is_some_and(|value| value.is_ascii_alphanumeric() || value == '_') {
            return false;
        }
        let tail = line[index + 1..].trim_start();
        return tail.starts_with('.') || matches!(tail.chars().next(), Some(',' | ';' | ')' | ']'));
    });
}

fn dart_top_mutable(line: &str) -> bool {
    if line.chars().next().is_some_and(char::is_whitespace) {
        return false;
    }
    let trimmed = line.trim_start();
    if ["final", "const", "class", "enum", "typedef", "import", "export", "part", "abstract", "mixin", "extension", "void", "Future", "@"]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return false;
    }
    return line.contains('=') && (declaration(line, "var") || line.split('=').next().is_some_and(|left| left.split_whitespace().count() >= 2));
}

fn dart_nonfinal_field(line: &str) -> bool {
    if !line.starts_with("  ") {
        return false;
    }
    let trimmed = line.trim_start();
    if ["final", "const", "static const", "static final", "@"]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return false;
    }
    let first = trimmed.chars().next();
    return first.is_some_and(|ch| ch.is_ascii_uppercase()) && (trimmed.contains('=') || trimmed.ends_with(';'));
}

fn dart_late(line: &str) -> bool {
    let Some(index) = line.find("late ") else {
        return false;
    };
    if !word_boundaries(line, index, "late".len()) {
        return false;
    }
    return !line[index + "late ".len()..].starts_with("final");
}

fn wildcard_arm(line: &str) -> bool {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('_') {
        return false;
    }
    return trimmed[1..].trim_start().starts_with("=>");
}

fn is_global_lock(line: &str) -> bool {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
    let Some(rest) = rest.strip_prefix("static ") else {
        return false;
    };
    let Some((name, ty)) = rest.split_once(':') else {
        return false;
    };
    let valid_name = !name.trim().is_empty()
        && name.trim().chars().all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_');
    return valid_name && contains_any(ty, &["Mutex", "RwLock", "RefCell", "Cell", "AtomicUsize", "AtomicBool", "OnceCell", "Lazy"]);
}

fn untyped_error(line: &str) -> bool {
    let compact = line.replace(' ', "");
    return compact.contains("Box<dynstd::error::Error")
        || compact.contains("Box<dynError")
        || line.match_indices("anyhow::Result").any(|(index, _)| colon_word_boundary(line, index))
        || line.match_indices("anyhow::Error").any(|(index, _)| colon_word_boundary(line, index));
}

fn colon_word_boundary(line: &str, index: usize) -> bool {
    let before = line[..index].chars().next_back();
    return before.is_none_or(|ch| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == ':'));
}

fn contains_interior_mutability(line: &str) -> bool {
    let compact = line.replace(' ', "");
    return compact.contains("RefCell<") || compact.contains("Cell<");
}
