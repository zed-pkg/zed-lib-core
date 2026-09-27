fn strip_noise(raw: Vec<String>) -> (Vec<String>, BTreeSet<usize>) {
    let mut lines = Vec::with_capacity(raw.len());
    let mut code_lines = BTreeSet::new();
    let mut in_block = false;
    for (offset, raw_line) in raw.into_iter().enumerate() {
        let (without_comments, next_in_block) = strip_comments(&raw_line, in_block);
        in_block = next_in_block;
        let masked = mask_strings(&without_comments);
        if !masked.trim().is_empty() {
            code_lines.insert(offset.saturating_add(1));
        }
        lines.push(masked);
    }
    return (lines, code_lines);
}

fn strip_comments(line: &str, initially_in_block: bool) -> (String, bool) {
    let bytes = line.as_bytes();
    let mut output = String::new();
    let mut index = 0_usize;
    let mut in_block = initially_in_block;
    while index < bytes.len() {
        if in_block {
            if index + 1 < bytes.len() && bytes[index] == b'*' && bytes[index + 1] == b'/' {
                output.push(' ');
                output.push(' ');
                index += 2;
                in_block = false;
                continue;
            }
            output.push(' ');
            index += 1;
            continue;
        }
        if index + 1 < bytes.len() && bytes[index] == b'/' && bytes[index + 1] == b'*' {
            output.push(' ');
            output.push(' ');
            index += 2;
            in_block = true;
            continue;
        }
        if index + 1 < bytes.len() && bytes[index] == b'/' && bytes[index + 1] == b'/' {
            let previous = if index == 0 { None } else { Some(bytes[index - 1]) };
            if previous != Some(b':') {
                break;
            }
        }
        output.push(char::from(bytes[index]));
        index += 1;
    }
    return (output, in_block);
}

fn mask_strings(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut output = String::new();
    let mut index = 0_usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if matches!(byte, b'"' | b'\'' | b'`') {
            let quote = byte;
            output.push('"');
            output.push('"');
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = index.saturating_add(2);
                    continue;
                }
                if bytes[index] == quote {
                    index += 1;
                    break;
                }
                index += 1;
            }
            continue;
        }
        output.push(char::from(byte));
        index += 1;
    }
    return output;
}
