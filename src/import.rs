//! Bulk import of events or biographies from a pasted table (TSV/CSV,
//! including what a browser gives you when you copy a rendered HTML table
//! straight off a page) or, optionally, fetched directly from a URL.
//!
//! Kept in two halves on purpose: everything up to `EventDraft`/
//! `BiographyDraft` is pure text-in, structured-data-out and has no idea
//! `Document` or `Id` exist, so it is fully unit-testable without a window.
//! Only the dialog code in `forms.rs` turns a draft into a real `Event`/
//! `Biography`, because only it has a `Document` to mint fresh ids from.

use crate::model::HDate;

// ---------------------------------------------------------------------------
// Parsing pasted text into rows
// ---------------------------------------------------------------------------

pub struct ParsedTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Strip `[1]`-style reference markers Wikipedia tables are full of, and
/// trim surrounding whitespace. Depth-counted rather than a single
/// find-and-cut so a stray unmatched bracket cannot eat the rest of a cell.
fn clean_cell(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for c in s.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

fn split_tsv_row(line: &str) -> Vec<String> {
    line.split('\t').map(clean_cell).collect()
}

/// A minimal quote-aware CSV splitter: commas inside `"..."` don't split the
/// field, and `""` inside quotes is an escaped literal quote.
fn split_csv_row(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields.into_iter().map(|f| clean_cell(&f)).collect()
}

/// Parse pasted table text into headers + rows. Tab-separated if any line
/// has a tab in it (what pasting a rendered HTML table into a text field
/// normally yields), comma-separated otherwise. The first non-blank line is
/// always the header row.
pub fn parse_table_text(text: &str) -> ParsedTable {
    let is_tsv = text.contains('\t');
    let split: fn(&str) -> Vec<String> = if is_tsv { split_tsv_row } else { split_csv_row };
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let headers = lines.next().map(split).unwrap_or_default();
    let rows: Vec<Vec<String>> = lines.map(split).collect();
    ParsedTable { headers, rows }
}

// ---------------------------------------------------------------------------
// HTML table extraction (for the "load from URL" path)
// ---------------------------------------------------------------------------

/// Fetch a URL and return its body as text. The one place in the app that
/// touches the network — opt-in, only ever called from the import dialog's
/// explicit "Von URL laden" button, never on startup or automatically.
pub fn fetch_url(url: &str) -> Result<String, String> {
    let mut response = ureq::get(url)
        .call()
        .map_err(|e| format!("Abruf von {url} fehlgeschlagen: {e}"))?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Antwort konnte nicht gelesen werden: {e}"))
}

/// Find the first table in `html` — preferring one with Wikipedia's
/// `wikitable` class, since an article page usually has several unrelated
/// `<table>`s (navigation boxes, infoboxes) before the data table itself —
/// and return it as the same tab-separated text `parse_table_text` expects.
///
/// Row/column spans are not reconstructed: a cell that visually spans
/// several rows only ends up in the first of them. Most simple listing
/// tables (a monarch's reign dates, say) don't use spans at all; a table
/// that does is better fixed up by hand after import than silently
/// misaligned by a best-effort guess here.
pub fn extract_first_table_as_tsv(html: &str) -> Result<String, String> {
    let document = scraper::Html::parse_document(html);
    let table_sel = scraper::Selector::parse("table").unwrap();
    let row_sel = scraper::Selector::parse("tr").unwrap();
    let cell_sel = scraper::Selector::parse("td, th").unwrap();

    let tables: Vec<_> = document.select(&table_sel).collect();
    let chosen = tables
        .iter()
        .find(|t| t.value().classes().any(|c| c == "wikitable"))
        .or_else(|| tables.first())
        .ok_or_else(|| "Auf dieser Seite wurde keine Tabelle gefunden.".to_string())?;

    let lines: Vec<String> = chosen
        .select(&row_sel)
        .filter_map(|row| {
            let cells: Vec<String> = row
                .select(&cell_sel)
                .map(|c| clean_cell(&c.text().collect::<Vec<_>>().join(" ")))
                .collect();
            (!cells.is_empty() && cells.iter().any(|c| !c.is_empty())).then(|| cells.join("\t"))
        })
        .collect();

    if lines.is_empty() {
        return Err("Die gefundene Tabelle enthält keine lesbaren Zeilen.".into());
    }
    Ok(lines.join("\n"))
}

// ---------------------------------------------------------------------------
// Rows -> drafts
// ---------------------------------------------------------------------------

/// Which detected column feeds which event field. Indices into
/// `ParsedTable::headers`/each row.
pub struct EventColumnMap {
    pub title: usize,
    pub date: usize,
    pub end_date: Option<usize>,
    pub description: Option<usize>,
}

pub struct EventDraft {
    pub title: String,
    pub start: HDate,
    pub end: Option<HDate>,
    pub description: String,
}

/// Which detected column feeds which biography field.
pub struct BiographyColumnMap {
    pub name: usize,
    pub birth: usize,
    pub death: Option<usize>,
    /// Matched against existing categories by name (case-insensitive); a
    /// name with no existing match becomes a new category.
    pub category: Option<usize>,
    /// Matched against existing timelines by name (case-insensitive); a
    /// name with no existing match is left unset rather than guessed at.
    pub culture: Option<usize>,
}

pub struct BiographyDraft {
    pub name: String,
    pub birth: HDate,
    pub death: Option<HDate>,
    pub category_name: Option<String>,
    pub culture_name: Option<String>,
}

fn cell(row: &[String], idx: usize) -> &str {
    row.get(idx).map(String::as_str).unwrap_or("")
}

/// Rows that fail to parse (no title, or a date `HDate::parse` cannot read)
/// are skipped rather than aborting the whole import — a single "circa"
/// note in someone's death-date column shouldn't cost every other row. Each
/// skip is reported as `(1-based row number, why)` so the dialog can show
/// the user exactly what to fix or import by hand.
pub fn build_event_drafts(table: &ParsedTable, map: &EventColumnMap) -> (Vec<EventDraft>, Vec<(usize, String)>) {
    let mut drafts = Vec::new();
    let mut skipped = Vec::new();
    for (i, row) in table.rows.iter().enumerate() {
        let title = cell(row, map.title).trim().to_string();
        if title.is_empty() {
            skipped.push((i + 1, "kein Titel".to_string()));
            continue;
        }
        let Some(start) = HDate::parse(cell(row, map.date)) else {
            skipped.push((i + 1, format!("Datum \"{}\" nicht verstanden", cell(row, map.date))));
            continue;
        };
        let end = map.end_date.and_then(|idx| HDate::parse(cell(row, idx)));
        let description = map.description.map(|idx| cell(row, idx).trim().to_string()).unwrap_or_default();
        drafts.push(EventDraft { title, start, end, description });
    }
    (drafts, skipped)
}

pub fn build_biography_drafts(
    table: &ParsedTable,
    map: &BiographyColumnMap,
) -> (Vec<BiographyDraft>, Vec<(usize, String)>) {
    let mut drafts = Vec::new();
    let mut skipped = Vec::new();
    for (i, row) in table.rows.iter().enumerate() {
        let name = cell(row, map.name).trim().to_string();
        if name.is_empty() {
            skipped.push((i + 1, "kein Name".to_string()));
            continue;
        }
        let Some(birth) = HDate::parse(cell(row, map.birth)) else {
            skipped.push((i + 1, format!("Geburtsdatum \"{}\" nicht verstanden", cell(row, map.birth))));
            continue;
        };
        let death = map.death.and_then(|idx| HDate::parse(cell(row, idx)));
        let non_empty = |idx: Option<usize>| {
            idx.map(|i| cell(row, i).trim().to_string()).filter(|s| !s.is_empty())
        };
        drafts.push(BiographyDraft {
            name,
            birth,
            death,
            category_name: non_empty(map.category),
            culture_name: non_empty(map.culture),
        });
    }
    (drafts, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tab_separated_paste_with_a_header_row() {
        let text = "Name\tYear\nAugustus\t-27\nTiberius\t14";
        let t = parse_table_text(text);
        assert_eq!(t.headers, vec!["Name", "Year"]);
        assert_eq!(t.rows, vec![vec!["Augustus", "-27"], vec!["Tiberius", "14"]]);
    }

    #[test]
    fn parses_comma_separated_text_with_quoted_fields() {
        let text = "Name,Notes\n\"Smith, John\",\"Said \"\"hi\"\"\"";
        let t = parse_table_text(text);
        assert_eq!(t.rows, vec![vec!["Smith, John", "Said \"hi\""]]);
    }

    #[test]
    fn strips_wikipedia_style_reference_brackets() {
        assert_eq!(clean_cell("Augustus[1][note 2]  "), "Augustus");
        assert_eq!(clean_cell("27 BC[citation needed]"), "27 BC");
    }

    #[test]
    fn extracts_the_wikitable_over_an_unrelated_navigation_table() {
        let html = r#"
            <table class="navbox"><tr><td>Nav</td></tr></table>
            <table class="wikitable">
                <tr><th>Emperor</th><th>Reign start</th></tr>
                <tr><td>Augustus</td><td>27 BC</td></tr>
                <tr><td>Tiberius</td><td>14</td></tr>
            </table>
        "#;
        let tsv = extract_first_table_as_tsv(html).unwrap();
        let table = parse_table_text(&tsv);
        assert_eq!(table.headers, vec!["Emperor", "Reign start"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0], vec!["Augustus", "27 BC"]);
    }

    #[test]
    fn missing_table_is_reported_rather_than_silently_returning_nothing() {
        assert!(extract_first_table_as_tsv("<p>no tables here</p>").is_err());
    }

    #[test]
    fn event_drafts_skip_rows_with_an_unreadable_date_but_keep_the_rest() {
        let table = ParsedTable {
            headers: vec!["Title".into(), "Year".into()],
            rows: vec![
                vec!["Battle of Actium".into(), "-31".into()],
                vec!["Something odd".into(), "not a date".into()],
                vec!["Death of Augustus".into(), "14".into()],
            ],
        };
        let map = EventColumnMap { title: 0, date: 1, end_date: None, description: None };
        let (drafts, skipped) = build_event_drafts(&table, &map);
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].title, "Battle of Actium");
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].0, 2, "the skipped row's 1-based index must point at row 2");
    }

    #[test]
    fn event_drafts_skip_rows_with_no_title() {
        let table = ParsedTable {
            headers: vec!["Title".into(), "Year".into()],
            rows: vec![vec![String::new(), "14".into()]],
        };
        let map = EventColumnMap { title: 0, date: 1, end_date: None, description: None };
        let (drafts, skipped) = build_event_drafts(&table, &map);
        assert!(drafts.is_empty());
        assert_eq!(skipped.len(), 1);
    }

    #[test]
    fn biography_drafts_carry_optional_category_and_culture_names() {
        let table = ParsedTable {
            headers: vec!["Name".into(), "Born".into(), "Died".into(), "Role".into(), "Empire".into()],
            rows: vec![vec![
                "Augustus".into(),
                "-63".into(),
                "14".into(),
                "Emperor".into(),
                "Roman Empire".into(),
            ]],
        };
        let map = BiographyColumnMap { name: 0, birth: 1, death: Some(2), category: Some(3), culture: Some(4) };
        let (drafts, skipped) = build_biography_drafts(&table, &map);
        assert!(skipped.is_empty());
        assert_eq!(drafts[0].category_name.as_deref(), Some("Emperor"));
        assert_eq!(drafts[0].culture_name.as_deref(), Some("Roman Empire"));
        assert_eq!(drafts[0].death.unwrap().year, 14);
    }
}
