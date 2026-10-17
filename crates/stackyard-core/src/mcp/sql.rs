//! Checks SQL from AI tools before the `mysql` client runs it. The client
//! reads its own commands from what it is given (`\!` and `system` run
//! programs, `source` reads files), so text outside quotes is put on one
//! line and backslashes there are refused. Queries that only read must be
//! one statement that starts with SELECT, SHOW, DESCRIBE or EXPLAIN, and
//! run in a read-only transaction besides. Changes may be several
//! statements but never drop a whole database. Neither may read or write
//! files on the computer.

/// What starts a query that only reads.
const READS: &[&str] = &["SELECT", "SHOW", "DESCRIBE", "DESC", "EXPLAIN", "WITH"];

/// The client's own commands, which it only reads at the start of a line.
const CLIENT_COMMANDS: &[&str] = &[
    "CHARSET",
    "CLEAR",
    "CONNECT",
    "DELIMITER",
    "EDIT",
    "EGO",
    "EXIT",
    "GO",
    "HELP",
    "NOPAGER",
    "NOTEE",
    "NOWARNING",
    "PAGER",
    "PRINT",
    "PROMPT",
    "QUIT",
    "REHASH",
    "RESETCONNECTION",
    "SOURCE",
    "STATUS",
    "SYSTEM",
    "TEE",
    "WARNINGS",
    "QUERY_ATTRIBUTES",
    "SSL_SESSION_DATA_PRINT",
];

/// One statement: its text, and its words outside quotes, in capitals.
struct Statement {
    text: String,
    words: Vec<String>,
}

/// The SQL to run for a query that only reads.
pub fn read_only(sql: &str) -> Result<String, String> {
    let statements = split(sql)?;
    let [statement] = statements.as_slice() else {
        return Err("send one statement at a time".into());
    };
    let first = statement.words.first().map(String::as_str).unwrap_or("");
    if !READS.contains(&first) {
        return Err(format!(
            "this tool only reads: start with {}; changes need change_database",
            READS.join(", ")
        ));
    }
    no_files(statement)?;
    Ok(format!(
        "SET SESSION TRANSACTION READ ONLY; {}",
        statement.text
    ))
}

/// The SQL to run for changes, followed by a count of the rows the last
/// statement changed.
pub fn changes(sql: &str) -> Result<String, String> {
    let statements = split(sql)?;
    if statements.is_empty() {
        return Err("there is no SQL to run".into());
    }
    let first = statements[0]
        .words
        .first()
        .map(String::as_str)
        .unwrap_or("");
    if CLIENT_COMMANDS.contains(&first) {
        return Err(format!("{first} is not SQL"));
    }
    for statement in &statements {
        no_files(statement)?;
        let drops_database = statement
            .words
            .windows(2)
            .any(|w| w[0] == "DROP" && (w[1] == "DATABASE" || w[1] == "SCHEMA"));
        if drops_database {
            return Err(
                "dropping a whole database is never allowed; the user can do that on the Databases screen"
                    .into(),
            );
        }
    }
    let texts: Vec<&str> = statements.iter().map(|s| s.text.as_str()).collect();
    Ok(format!(
        "{}; SELECT ROW_COUNT() AS changed_rows",
        texts.join("; ")
    ))
}

fn no_files(statement: &Statement) -> Result<(), String> {
    for word in ["OUTFILE", "DUMPFILE", "INFILE", "LOAD_FILE"] {
        if statement.words.iter().any(|w| w == word) {
            return Err(format!(
                "{word} reads or writes files, so it is not allowed"
            ));
        }
    }
    Ok(())
}

/// Splits SQL into statements, dropping comments and putting everything
/// outside quotes on one line.
fn split(sql: &str) -> Result<Vec<Statement>, String> {
    let mut statements = Vec::new();
    let mut text = String::new();
    let mut words = Vec::new();
    let mut word = String::new();
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0;
    let end_word = |word: &mut String, words: &mut Vec<String>| {
        if !word.is_empty() {
            words.push(std::mem::take(word).to_ascii_uppercase());
        }
    };
    let space = |text: &mut String| {
        if !text.is_empty() && !text.ends_with(' ') {
            text.push(' ');
        }
    };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            '\'' | '"' | '`' => {
                end_word(&mut word, &mut words);
                let start = i;
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err("a quote is not closed".into()),
                        Some('\\') if c != '`' => i += 2,
                        // A doubled quote stands for itself.
                        Some(&q) if q == c && chars.get(i + 1) == Some(&c) => i += 2,
                        Some(&q) if q == c => break,
                        Some(_) => i += 1,
                    }
                }
                text.extend(&chars[start..=i.min(chars.len() - 1)]);
                i += 1;
                continue;
            }
            '/' if next == Some('*') => {
                if matches!(chars.get(i + 2), Some('!' | '+')) {
                    return Err("comments that run as SQL (/*! … */) are not allowed".into());
                }
                end_word(&mut word, &mut words);
                let close = (i + 2..chars.len().saturating_sub(1))
                    .find(|&j| chars[j] == '*' && chars[j + 1] == '/')
                    .ok_or("a /* comment is not closed")?;
                space(&mut text);
                i = close + 2;
                continue;
            }
            '#' => {
                end_word(&mut word, &mut words);
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                space(&mut text);
                continue;
            }
            '-' if next == Some('-')
                && chars
                    .get(i + 2)
                    .is_none_or(|c| c.is_whitespace() || c.is_control()) =>
            {
                end_word(&mut word, &mut words);
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                space(&mut text);
                continue;
            }
            '\\' => return Err("backslashes are only allowed inside quotes".into()),
            ';' => {
                end_word(&mut word, &mut words);
                let done = text.trim().to_string();
                if !done.is_empty() {
                    statements.push(Statement {
                        text: done,
                        words: std::mem::take(&mut words),
                    });
                }
                text.clear();
                words.clear();
            }
            c if c.is_whitespace() || c.is_control() => {
                end_word(&mut word, &mut words);
                space(&mut text);
            }
            c => {
                if c.is_alphanumeric() || c == '_' || c == '$' {
                    word.push(c);
                } else {
                    end_word(&mut word, &mut words);
                }
                text.push(c);
            }
        }
        i += 1;
    }
    end_word(&mut word, &mut words);
    let done = text.trim().to_string();
    if !done.is_empty() {
        statements.push(Statement { text: done, words });
    }
    Ok(statements)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_one_statement_on_one_line() {
        assert_eq!(
            read_only("SELECT *\n  FROM users -- everyone\n WHERE name = 'a;\nb' ;").unwrap(),
            "SET SESSION TRANSACTION READ ONLY; SELECT * FROM users WHERE name = 'a;\nb'"
        );
        assert!(read_only("show tables").is_ok());
        // The client reads its own commands only at the start of a line.
        assert_eq!(
            read_only("SELECT 1\nsystem rm -rf /").unwrap(),
            "SET SESSION TRANSACTION READ ONLY; SELECT 1 system rm -rf /"
        );
        assert!(read_only("/* hi */ DESCRIBE `we``ird`").is_ok());
        for bad in [
            "",
            "SELECT 1; SELECT 2",
            "DELETE FROM users",
            "UPDATE users SET a = 1",
            "SELECT * FROM users INTO OUTFILE '/tmp/x'",
            "SELECT LOAD_FILE('/etc/passwd')",
            "SELECT 1 \\! rm -rf /",
            "SELECT /*! 1; DROP TABLE users */",
            "SELECT 'not closed",
            "SELECT 1 /* not closed",
        ] {
            assert!(read_only(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn changes_never_drop_databases_or_run_client_commands() {
        assert_eq!(
            changes("UPDATE users SET name = 'x'\nWHERE id = 1;\nINSERT INTO log VALUES (1);")
                .unwrap(),
            "UPDATE users SET name = 'x' WHERE id = 1; INSERT INTO log VALUES (1); SELECT ROW_COUNT() AS changed_rows"
        );
        assert!(changes("DROP TABLE old_things").is_ok());
        // Words inside quotes are only text.
        assert!(changes("INSERT INTO notes VALUES ('drop database shop')").is_ok());
        for bad in [
            "",
            "  ;  ",
            "DROP DATABASE shop",
            "drop /* x */ schema `shop`",
            "UPDATE a SET b = 1; DROP DATABASE shop",
            "system rm -rf /",
            "source /etc/passwd",
            "LOAD DATA LOCAL INFILE '/etc/passwd' INTO TABLE t",
            "UPDATE a SET b = 1 \\g",
        ] {
            assert!(changes(bad).is_err(), "{bad}");
        }
    }
}
