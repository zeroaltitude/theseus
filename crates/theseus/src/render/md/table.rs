//! Markdown tables (theseus-rn7s): aligned columns in box lines, cut to the
//! width with a marker where they are too wide, and never wrapped inside a
//! cell.

use super::super::style::{Color, Mode, Span, Style, StyledLine};
use super::block::{cells, delimiter, Align};
use super::inline::inline;

/// A column is never cut narrower than this.
const MIN_COL: usize = 3;

/// The table's lines for a terminal `width` columns wide.
pub fn render(rows: &[String], width: usize, mode: Mode) -> Vec<StyledLine> {
    let aligns = delimiter(&rows[1]).unwrap_or_default();
    let n = aligns.len().max(1);
    let grid: Vec<Vec<StyledLine>> = std::iter::once(&rows[0])
        .chain(rows[2..].iter())
        .enumerate()
        .map(|(r, row)| cell_lines(row, n, r == 0, mode))
        .collect();
    let widths = fit(&grid, n, width);
    let edge = Style::fg(Color::Gray);
    let mut out = vec![rule('┌', '┬', '┐', &widths, &edge)];
    for (r, row) in grid.iter().enumerate() {
        out.push(row_line(row, &widths, &aligns, &edge));
        if r == 0 {
            out.push(rule('├', '┼', '┤', &widths, &edge));
        }
    }
    out.push(rule('└', '┴', '┘', &widths, &edge));
    out.into_iter().map(|l| l.truncated(width)).collect()
}

/// One row's cells as styled lines, the header's in bold; the row made `n`
/// cells long.
fn cell_lines(row: &str, n: usize, header: bool, mode: Mode) -> Vec<StyledLine> {
    let base = if header {
        Style::bold()
    } else {
        Style::default()
    };
    let mut cs = cells(row);
    cs.resize(n, String::new());
    cs.iter()
        .map(|c| StyledLine::new(inline(c, &base, mode).into_iter().map(flat).collect()))
        .collect()
}

/// A span with its line breaks as spaces: a cell is one line.
fn flat(s: Span) -> Span {
    Span::new(s.text.replace('\n', " "), s.style)
}

/// Column widths: as wide as the widest cell, the widest columns narrowed one
/// column at a time until the table fits.
fn fit(grid: &[Vec<StyledLine>], n: usize, width: usize) -> Vec<usize> {
    let mut w: Vec<usize> = (0..n)
        .map(|c| grid.iter().map(|r| r[c].width()).max().unwrap_or(0).max(1))
        .collect();
    let frame = 3 * n + 1;
    while w.iter().sum::<usize>() + frame > width {
        let widest = (0..n).max_by_key(|c| w[*c]).unwrap_or(0);
        if w[widest] <= MIN_COL {
            break;
        }
        w[widest] -= 1;
    }
    w
}

fn rule(left: char, mid: char, right: char, widths: &[usize], edge: &Style) -> StyledLine {
    let mut s = String::new();
    s.push(left);
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            s.push(mid);
        }
        s.push_str(&"─".repeat(w + 2));
    }
    s.push(right);
    StyledLine::from_text(s, edge.clone())
}

fn row_line(row: &[StyledLine], widths: &[usize], aligns: &[Align], edge: &Style) -> StyledLine {
    let mut line = StyledLine::from_text("│", edge.clone());
    for (c, cell) in row.iter().enumerate() {
        let cell = cell.truncated(widths[c]);
        let pad = widths[c] - cell.width();
        let (left, right) = match aligns.get(c).copied().unwrap_or(Align::Left) {
            Align::Left => (0, pad),
            Align::Right => (pad, 0),
            Align::Center => (pad / 2, pad - pad / 2),
        };
        line.push(&format!(" {}", " ".repeat(left)), &Style::default());
        line.extend(cell);
        line.push(&format!("{} ", " ".repeat(right)), &Style::default());
        line.push("│", edge);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[&str], width: usize) -> Vec<String> {
        let rows: Vec<String> = rows.iter().map(ToString::to_string).collect();
        render(&rows, width, Mode::Plain)
            .iter()
            .map(StyledLine::plain)
            .collect()
    }

    #[test]
    fn a_table_is_aligned_in_box_lines() {
        let t = table(
            &["| name | n |", "|:--|--:|", "| a | 10 |", "| **bb** | 2 |"],
            40,
        );
        assert_eq!(
            t,
            [
                "┌──────┬────┐",
                "│ name │  n │",
                "├──────┼────┤",
                "│ a    │ 10 │",
                "│ bb   │  2 │",
                "└──────┴────┘",
            ]
        );
    }

    #[test]
    fn a_wide_table_is_cut_in_its_cells_never_wrapped() {
        let t = table(
            &[
                "| key | description |",
                "|--|--|",
                "| a | a very long description indeed |",
            ],
            24,
        );
        assert!(
            t.iter().all(|l| crate::render::style::text_width(l) <= 24),
            "{t:?}"
        );
        assert_eq!(t.len(), 5);
        assert!(t[3].contains('…'), "{t:?}");
        // Many columns that cannot fit at all are cut at the line.
        let many = table(
            &[
                "|a|b|c|d|e|f|g|h|",
                "|-|-|-|-|-|-|-|-|",
                "|1|2|3|4|5|6|7|8|",
            ],
            12,
        );
        assert!(
            many.iter()
                .all(|l| crate::render::style::text_width(l) <= 12),
            "{many:?}"
        );
    }

    #[test]
    fn short_and_long_rows_are_made_as_wide_as_the_header() {
        let t = table(&["| a | b |", "|-|-|", "| 1 |", "| 1 | 2 | 3 |"], 40);
        assert_eq!(t[3], "│ 1 │   │");
        assert_eq!(t[4], "│ 1 │ 2 │");
    }
}
