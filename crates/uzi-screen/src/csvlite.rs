//! Minimal RFC 4180 reader matching Python's `csv` module for the two places
//! the screening layer parses delimited text (`portfolio_runner._parse_csv` and
//! `sources.parse_minutes`). The workspace has no CSV crate; the semantics that
//! matter here (quoted separators, `""` escapes, quoted embedded newlines,
//! `\r\n`) are reproduced directly.

/// Parse a whole document into records (Python `csv.reader`). A trailing
/// newline does not produce a final empty record.
pub(crate) fn parse_records(text: &str) -> Vec<Vec<String>> {
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut field_started = false;
    let mut record_started = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        field.push('"');
                        chars.next();
                    } else {
                        in_quotes = false;
                    }
                }
                other => field.push(other),
            }
            continue;
        }
        match c {
            '"' if !field_started => {
                in_quotes = true;
                field_started = true;
                record_started = true;
            }
            ',' => {
                record.push(std::mem::take(&mut field));
                field_started = false;
                record_started = true;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                field_started = false;
                record_started = false;
            }
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                field_started = false;
                record_started = false;
            }
            other => {
                field.push(other);
                field_started = true;
                record_started = true;
            }
        }
    }
    if record_started || field_started || !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

/// Parse a single CSV record (Python `next(csv.reader([line]))`).
pub(crate) fn parse_line(line: &str) -> Vec<String> {
    parse_records(line).into_iter().next().unwrap_or_default()
}

/// Python `encoding="utf-8-sig"` — drop a leading BOM.
pub(crate) fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_fields_and_escapes() {
        assert_eq!(
            parse_records("a,\"b,c\",\"d\"\"e\"\r\nf,g,h\n"),
            vec![
                vec!["a".to_string(), "b,c".to_string(), "d\"e".to_string()],
                vec!["f".to_string(), "g".to_string(), "h".to_string()],
            ]
        );
    }

    #[test]
    fn quoted_newline_stays_in_one_field() {
        let rows = parse_records("x,\"line1\nline2\"\ny,z\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][1], "line1\nline2");
    }

    #[test]
    fn trailing_newline_does_not_add_a_record() {
        assert_eq!(parse_records("\n\n").len(), 2);
        assert_eq!(parse_records("").len(), 0);
    }

    #[test]
    fn empty_fields_are_preserved() {
        assert_eq!(parse_line("a,,c"), vec!["a", "", "c"]);
        assert_eq!(parse_line("a,"), vec!["a", ""]);
    }
}
