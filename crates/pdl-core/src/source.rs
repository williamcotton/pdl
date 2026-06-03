use crate::Diagnostic;

pub fn line_col(source: &str, byte_offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;

    for (idx, ch) in source.char_indices() {
        if idx >= byte_offset {
            break;
        }

        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }

    (line, col)
}

pub fn render_diagnostic(source_name: &str, source: &str, diagnostic: &Diagnostic) -> String {
    let (line, col) = line_col(source, diagnostic.span.start);
    format!(
        "{}[{}] {}:{}:{}: {}",
        diagnostic.severity, diagnostic.code, source_name, line, col, diagnostic.message
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_uses_byte_offsets() {
        assert_eq!(line_col("a\néz", 3), (2, 2));
    }
}
