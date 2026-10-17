//! Reads what `psql --html` prints: one `<table>` per statement that
//! returned rows, a first `<tr>` of `<th>` column names, then one `<tr>`
//! of `<td>` cells per row. psql escapes `& < > "`, writes a line break as
//! `<br />`, a leading space as `&nbsp;` and an empty value (or one of only spaces)
//! as `&nbsp; `.
//! NULL is printed as the marker the caller chose. This is the only HTML
//! psql writes, so this reader handles exactly that and nothing more.
//! Output cut off part-way keeps every row that was complete.

use super::ResultSet;

pub fn parse(bytes: &[u8], null: &str) -> Vec<ResultSet> {
    let text = String::from_utf8_lossy(bytes);
    let mut sets = Vec::new();
    let mut rest: &str = &text;
    while let Some(start) = rest.find("<table") {
        rest = &rest[start..];
        let (body, complete) = match rest.find("</table>") {
            Some(end) => (&rest[..end], true),
            None => (rest, false),
        };
        let mut set = ResultSet::default();
        let mut rows: &str = body;
        while let Some(open) = rows.find("<tr") {
            rows = &rows[open..];
            let Some(close) = rows.find("</tr>") else {
                break;
            };
            let cells = cells(&rows[..close], null);
            if set.columns.is_empty() && rows[..close].contains("<th") {
                set.columns = cells.into_iter().map(Option::unwrap_or_default).collect();
            } else {
                set.rows.push(cells);
            }
            rows = &rows[close + "</tr>".len()..];
        }
        sets.push(set);
        if !complete {
            break;
        }
        rest = &rest[body.len() + "</table>".len()..];
    }
    sets
}

fn cells(row: &str, null: &str) -> Vec<Option<String>> {
    let mut out = Vec::new();
    let mut rest = row;
    while let Some(open) = rest.find(['<']) {
        rest = &rest[open..];
        let is_cell = rest.starts_with("<td") || rest.starts_with("<th");
        let Some(tag_end) = rest.find('>') else { break };
        if !is_cell {
            rest = &rest[tag_end + 1..];
            continue;
        }
        let closing = if rest.starts_with("<td") {
            "</td>"
        } else {
            "</th>"
        };
        let after = &rest[tag_end + 1..];
        let Some(close) = after.find(closing) else {
            break;
        };
        let value = unescape(&after[..close]);
        out.push((value != null).then_some(value));
        rest = &after[close + closing.len()..];
    }
    out
}

fn unescape(raw: &str) -> String {
    if raw == "&nbsp; " {
        return String::new();
    }
    raw.replace("<br />\n", "\n")
        .replace("<br />", "\n")
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NULL: &str = "\u{1}null-1";

    #[test]
    fn reads_tables_cells_and_nulls() {
        let out = format!(
            "<table border=\"1\">\n  <tr>\n    <th align=\"center\">id</th>\n    <th align=\"center\">name</th>\n    <th align=\"center\">note</th>\n  </tr>\n\
             \x20 <tr valign=\"top\">\n    <td align=\"right\">1</td>\n    <td align=\"left\">Ana &amp; &lt;Bo&gt; &quot;x&quot;</td>\n    <td align=\"left\">{NULL}</td>\n  </tr>\n\
             \x20 <tr valign=\"top\">\n    <td align=\"right\">2</td>\n    <td align=\"left\">line<br />\nbreak</td>\n    <td align=\"left\">&nbsp; </td>\n  </tr>\n</table>\n\
             <table border=\"1\">\n  <tr>\n    <th align=\"center\">n</th>\n  </tr>\n</table>\n"
        );
        let sets = parse(out.as_bytes(), NULL);
        assert_eq!(sets.len(), 2);
        assert_eq!(sets[0].columns, ["id", "name", "note"]);
        assert_eq!(
            sets[0].rows[0],
            [Some("1".into()), Some("Ana & <Bo> \"x\"".into()), None]
        );
        assert_eq!(sets[0].rows[1][1].as_deref(), Some("line\nbreak"));
        assert_eq!(sets[0].rows[1][2].as_deref(), Some(""));
        assert_eq!(sets[1].columns, ["n"]);
        assert!(sets[1].rows.is_empty());
        // A leading space stays.
        let sets = parse(
            b"<table><tr><th>a</th></tr><tr><td>&nbsp;x &amp;nbsp;</td></tr></table>",
            NULL,
        );
        assert_eq!(sets[0].rows[0][0].as_deref(), Some(" x &nbsp;"));
    }

    #[test]
    fn keeps_complete_rows_of_cut_off_output() {
        let out = "<table><tr><th>a</th></tr><tr><td>1</td></tr><tr><td>2</t";
        let sets = parse(out.as_bytes(), NULL);
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].rows, [[Some("1".to_string())]]);
        assert!(parse(b"INSERT 0 1\n", NULL).is_empty());
    }
}
