//! Reads what `mysql --xml` prints: one `<resultset>` per statement that
//! returned rows, each `<row>` holding `<field name="…">value</field>`, or
//! `<field name="…" xsi:nil="true" />` for NULL. This is the only XML the
//! client writes, so this reader handles exactly that and nothing more.
//! Output cut off part-way keeps every row that was complete.

use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ResultSet {
    pub columns: Vec<String>,
    /// `None` is SQL NULL.
    pub rows: Vec<Vec<Option<String>>>,
    /// More rows were left out than are shown.
    pub truncated: bool,
}

pub fn parse(bytes: &[u8]) -> Vec<ResultSet> {
    let text = String::from_utf8_lossy(bytes);
    let mut sets = Vec::new();
    let mut set: Option<ResultSet> = None;
    let mut row: Option<(Vec<String>, Vec<Option<String>>)> = None;
    let mut rest: &str = &text;
    while let Some(start) = rest.find('<') {
        rest = &rest[start..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[1..end];
        let after = &rest[end + 1..];
        let closing = tag.starts_with('/');
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        match (closing, name) {
            (false, "resultset") => set = Some(ResultSet::default()),
            (true, "resultset") => sets.extend(set.take()),
            (false, "row") => row = Some((Vec::new(), Vec::new())),
            (true, "row") => {
                if let (Some(set), Some((names, values))) = (set.as_mut(), row.take()) {
                    if set.columns.is_empty() {
                        set.columns = names;
                    }
                    set.rows.push(values);
                }
            }
            (false, "field") => {
                let column = attribute(tag, "name").unwrap_or_default();
                let value = if tag.ends_with('/') {
                    // `<field name="x" xsi:nil="true" />` is NULL; `<field name="x"/>` is ''.
                    (attribute(tag, "xsi:nil").as_deref() != Some("true")).then(String::new)
                } else {
                    let Some(close) = after.find("</field>") else { break };
                    let value = unescape(&after[..close]);
                    rest = &after[close + "</field>".len()..];
                    if let Some((names, values)) = row.as_mut() {
                        names.push(column);
                        values.push(Some(value));
                    }
                    continue;
                };
                if let Some((names, values)) = row.as_mut() {
                    names.push(column);
                    values.push(value);
                }
            }
            _ => {}
        }
        rest = after;
    }
    // A cut-off output still has its last result's complete rows.
    sets.extend(set);
    sets
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let len = tag[start..].find('"')?;
    Some(unescape(&tag[start..start + len]))
}

fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>

<resultset statement="SELECT id, name, note FROM people WHERE name &lt;&gt; &apos;x&apos;
" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <row>
	<field name="id">1</field>
	<field name="name">Ana &amp; &quot;Bo&quot; &lt;b&gt;</field>
	<field name="note" xsi:nil="true" />
  </row>

  <row>
	<field name="id">2</field>
	<field name="name">two
lines &#233;</field>
	<field name="note"></field>
  </row>
</resultset>
<?xml version="1.0"?>

<resultset statement="SELECT 1 AS one
" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <row>
	<field name="one">1</field>
  </row>
</resultset>
"#;

    #[test]
    fn reads_rows_nulls_and_entities() {
        let sets = parse(SAMPLE.as_bytes());
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[0].columns, ["id", "name", "note"]);
        assert_eq!(
            sets[0].rows,
            [
                vec![Some("1".into()), Some("Ana & \"Bo\" <b>".into()), None],
                vec![Some("2".into()), Some("two\nlines é".into()), Some(String::new())],
            ]
        );
        assert_eq!(sets[1].rows, [vec![Some("1".to_string())]]);
    }

    #[test]
    fn an_empty_result_has_no_columns() {
        let text = "<?xml version=\"1.0\"?>\n<resultset statement=\"SELECT * FROM t\" xmlns:xsi=\"x\">\n</resultset>\n";
        let sets = parse(text.as_bytes());
        assert_eq!(sets, [ResultSet::default()]);
        assert!(parse(b"").is_empty());
    }

    #[test]
    fn keeps_complete_rows_of_cut_off_output() {
        let cut = &SAMPLE[..SAMPLE.find("two").unwrap()];
        let sets = parse(cut.as_bytes());
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].rows.len(), 1);
    }

    #[test]
    fn stray_ampersands_stay() {
        assert_eq!(unescape("a & b &bogus; &#xZZ; &#65;"), "a & b &bogus; &#xZZ; A");
    }
}
