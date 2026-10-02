//! Drawing (design `stage2` §2.9's layout): the header, the sidebar with its
//! trees, the detail pane, and the footer, into a ratatui buffer. Nothing
//! here changes the app: the same app draws the same buffer, so tests
//! snapshot it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use theseus_client::render::{glyph, Tag};
use theseus_protocol::Level;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Link, Mode};
use crate::board::{Only, Row};

/// The done-until-seen mark (design §2.9): teal, between needs you and
/// working.
pub const DONE: char = '◆';

/// Draw the whole screen. Returns where the cursor goes, when a line is
/// being typed.
pub fn draw(app: &App, buf: &mut Buffer) -> Option<(u16, u16)> {
    let area = buf.area;
    if area.width < 20 || area.height < 4 {
        buf.set_stringn(
            0,
            0,
            "theseus: too small",
            area.width as usize,
            Style::default(),
        );
        return None;
    }
    header(app, Rect::new(0, 0, area.width, 1), buf);
    let body = Rect::new(0, 1, area.width, area.height - 2);
    let rows = app.rows();
    if app.wide() {
        let side = sidebar_width(area.width);
        sidebar(app, &rows, Rect::new(0, body.y, side, body.height), buf);
        for y in body.y..body.y + body.height {
            buf.set_string(side, y, "│", dim());
        }
        let right = Rect::new(side + 1, body.y, area.width - side - 1, body.height);
        empty_detail(right, buf);
    } else {
        sidebar(app, &rows, body, buf);
    }
    let cursor = footer(app, Rect::new(0, area.height - 1, area.width, 1), buf);
    if app.mode == Mode::Help {
        help(body, buf);
    }
    cursor
}

/// The sidebar's width beside the detail pane: three tenths of the screen,
/// from 36 to 56 columns.
pub fn sidebar_width(width: u16) -> u16 {
    (width * 3 / 10).clamp(36, 56)
}

fn header(app: &App, area: Rect, buf: &mut Buffer) {
    let (need, done) = app.board.counts(&|_| false);
    let mut left = String::from(" theseus");
    if need > 0 {
        left.push_str(&format!(
            " · {need} need{} you",
            if need == 1 { "s" } else { "" }
        ));
    }
    if done > 0 {
        left.push_str(&format!(" · {done} done"));
    }
    if need == 0 && done == 0 {
        let n = app.board.len();
        left.push_str(&format!(" · {n} session{}", if n == 1 { "" } else { "s" }));
    }
    if app.only != Only::All {
        left.push_str(&format!(" · only {}", app.only.word()));
    }
    if !app.filter.is_empty() {
        left.push_str(&format!(" · /{}", app.filter));
    }
    let link = match &app.link {
        Link::Connecting => "connecting".to_string(),
        Link::Up => "socket ok".to_string(),
        Link::Down { attempt, .. } => format!("reconnecting ({attempt})"),
    };
    let right = format!("{} · {link} ", (app.hm)(app.now_ms));
    let style = Style::default().add_modifier(Modifier::BOLD);
    buf.set_style(area, Style::default().bg(Color::Black));
    let room = (area.width as usize).saturating_sub(right.width() + 1);
    buf.set_stringn(area.x, area.y, &left, room, style);
    let link_style = match app.link {
        Link::Up => Style::default().fg(Color::Gray),
        _ => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    };
    let x = area.x + area.width.saturating_sub(right.width() as u16);
    buf.set_stringn(x, area.y, &right, right.width(), link_style);
}

fn sidebar(app: &App, rows: &[Row], area: Rect, buf: &mut Buffer) {
    if rows.is_empty() {
        let text = match (&app.link, app.only, app.filter.is_empty()) {
            (Link::Connecting, ..) => " connecting to theseusd…",
            (_, Only::All, true) => " no sessions yet",
            _ => " nothing matches (a: show all)",
        };
        buf.set_stringn(area.x, area.y, text, area.width as usize, dim());
        return;
    }
    let cursor = app.cursor(rows);
    // Keep the cursor's row on screen: scroll only as far as it needs.
    let height = area.height as usize;
    let top = match cursor {
        Some(c) if c >= height => c + 1 - height,
        _ => 0,
    };
    for (i, row) in rows.iter().enumerate().skip(top).take(height) {
        let y = area.y + (i - top) as u16;
        draw_row(
            row,
            Rect::new(area.x, y, area.width, 1),
            buf,
            Some(i) == cursor,
        );
    }
}

/// One row: ` ● confirm proc.run  DM        $0.42`, a task indented under its
/// parent (`└ ◐ turn 2  task b4c5d6`).
fn draw_row(row: &Row, area: Rect, buf: &mut Buffer, selected: bool) {
    let mut prefix = String::from(" ");
    if row.depth > 0 {
        prefix.push_str(&"  ".repeat(row.depth));
        prefix.push(if row.last { '└' } else { '├' });
        prefix.push(' ');
    }
    let mark = if row.done { DONE } else { glyph(row.level) };
    let cost = format!(" ${:.2} ", row.spent_usd);
    let mut name = row.name.clone();
    if row.folded > 0 {
        name.push_str(&format!(" +{}", row.folded));
    }
    let width = area.width as usize;
    let base = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    buf.set_style(area, base);
    let mut x = area.x;
    let room = width.saturating_sub(cost.width());
    let mut used = 0usize;
    let mut put = |text: &str, style: Style, x: &mut u16, used: &mut usize| {
        let n = room.saturating_sub(*used);
        let (nx, _) = buf.set_stringn(*x, area.y, text, n, base.patch(style));
        *used += (nx - *x) as usize;
        *x = nx;
    };
    put(&prefix, dim(), &mut x, &mut used);
    put(
        &format!("{mark} "),
        level_style(row.level, row.done),
        &mut x,
        &mut used,
    );
    let label_style = if row.level == Level::Idle && !row.done {
        dim()
    } else {
        Style::default()
    };
    put(&row.label, label_style, &mut x, &mut used);
    put("  ", Style::default(), &mut x, &mut used);
    put(&name, Style::default(), &mut x, &mut used);
    let cx = area.x + area.width.saturating_sub(cost.width() as u16);
    buf.set_stringn(cx, area.y, &cost, cost.width(), base.patch(dim()));
}

fn empty_detail(area: Rect, buf: &mut Buffer) {
    buf.set_stringn(
        area.x + 1,
        area.y,
        "no session open",
        area.width.saturating_sub(1) as usize,
        dim(),
    );
}

fn footer(app: &App, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
    let keys = "[?] help  [q] quit ";
    match app.mode {
        Mode::Filter => {
            let text = format!(" /{}", app.filter);
            buf.set_stringn(area.x, area.y, &text, area.width as usize, Style::default());
            let x = (text.width() as u16).min(area.width.saturating_sub(1));
            return Some((area.x + x, area.y));
        }
        Mode::Normal | Mode::Help => {}
    }
    let room = (area.width as usize).saturating_sub(keys.width() + 1);
    if let Some((tag, text)) = &app.flash {
        buf.set_stringn(area.x + 1, area.y, text, room, tag_style(*tag));
    }
    let x = area.x + area.width.saturating_sub(keys.width() as u16);
    buf.set_stringn(x, area.y, keys, keys.width(), dim());
    None
}

/// The keys (design §2.9), over the body.
fn help(area: Rect, buf: &mut Buffer) {
    const KEYS: [&str; 9] = [
        " keys",
        " ↑ ↓  j k   move",
        " / text     filter by text (enter keeps it, esc clears)",
        " b w r d a  only needs you, working, ready, done; all",
        " esc        clear the filters",
        " ?          this help",
        " q          quit (ctrl-c from anywhere)",
        "",
        " any key closes this",
    ];
    let w = KEYS.iter().map(|l| l.width()).max().unwrap_or(0) as u16 + 2;
    let h = KEYS.len() as u16;
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let rect = Rect::new(x, y, w.min(area.width), h.min(area.height));
    buf.set_style(rect, Style::default().bg(Color::Black));
    for (i, l) in KEYS.iter().enumerate().take(rect.height as usize) {
        let style = if i == 0 {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        // Clear the row first, so the rows beneath do not show through.
        buf.set_stringn(
            x,
            y + i as u16,
            " ".repeat(rect.width as usize),
            rect.width as usize,
            style,
        );
        buf.set_stringn(x, y + i as u16, l, rect.width as usize, style);
    }
}

pub fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// A level's color, as herdr colors it (re-implemented): needs you red, done
/// teal, working yellow, ready green, idle dim.
pub fn level_style(level: Level, done: bool) -> Style {
    if done {
        return Style::default().fg(Color::Cyan);
    }
    match level {
        Level::NeedsYou => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        Level::Working => Style::default().fg(Color::Yellow),
        Level::Ready => Style::default().fg(Color::Green),
        Level::Idle => dim(),
    }
}

/// How a line from the CLI's renderers looks, by its tag.
pub fn tag_style(tag: Tag) -> Style {
    match tag {
        Tag::Plain => Style::default(),
        Tag::Reply => Style::default().fg(Color::White),
        Tag::Thinking => dim().add_modifier(Modifier::ITALIC),
        Tag::Tool => Style::default().fg(Color::Blue),
        Tag::Ask => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        Tag::Ok => Style::default().fg(Color::Green),
        Tag::Warn => Style::default().fg(Color::Yellow),
        Tag::Bad => Style::default().fg(Color::Red),
        Tag::Dim => dim(),
        Tag::Level(l) => level_style(l, false),
    }
}

/// The screen as text, one string per row: what a test compares.
#[cfg(test)]
pub fn text(buf: &Buffer) -> Vec<String> {
    let a = buf.area;
    (a.y..a.y + a.height)
        .map(|y| {
            let mut s = String::new();
            for x in a.x..a.x + a.width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.trim_end().to_string()
        })
        .collect()
}
