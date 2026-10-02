//! Drawing (design `stage2` §2.9's layout): the header, the sidebar with its
//! trees, the detail pane, and the footer with the input line, into a ratatui
//! buffer. Nothing here changes the app: the same app draws the same buffer,
//! so tests snapshot it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use theseus_client::render::{glyph, pill, Tag};
use theseus_protocol::Level;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, Link, Mode};
use crate::board::{short, Only, Row};
use crate::card;

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
    if app.wide() {
        let side = sidebar_width(area.width);
        sidebar(app, Rect::new(0, body.y, side, body.height), buf);
        for y in body.y..body.y + body.height {
            buf.set_string(side, y, "│", dim());
        }
        let right = Rect::new(side + 1, body.y, area.width - side - 1, body.height);
        detail(app, right, buf);
    } else if app.shown().is_some() {
        // Below 100 columns the session in focus takes the screen; esc goes
        // back to the list.
        detail(app, body, buf);
    } else {
        sidebar(app, body, buf);
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
    let (need, done) = app.board.counts(&|v| app.is_done(v));
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

fn sidebar(app: &App, area: Rect, buf: &mut Buffer) {
    let rows = &app.rows();
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

/// The focused session's pane: a header line (its id, name, model, and
/// pill), then its history and events, the newest at the foot.
fn detail(app: &App, area: Rect, buf: &mut Buffer) {
    let width = area.width.saturating_sub(1) as usize;
    let x = area.x + 1;
    let Some(d) = &app.detail else {
        buf.set_stringn(
            x,
            area.y,
            "no session open: enter opens the one under the cursor",
            width,
            dim(),
        );
        return;
    };
    buf.set_stringn(
        x,
        area.y,
        detail_header(app, &d.session_id),
        width,
        Style::default().add_modifier(Modifier::BOLD),
    );
    let mut height = area.height.saturating_sub(1) as usize;
    // The card: the session's first question, at the pane's foot, under a
    // rule (design §2.9, "Answering").
    let card: Vec<(Tag, String)> = app
        .card()
        .map(|(q, more)| {
            card::lines(
                q,
                app.now_ms,
                app.refused.get(q.correlation_id()).map(String::as_str),
                more,
            )
        })
        .unwrap_or_default();
    if !card.is_empty() {
        let card_h = (card.len() + 1).min(height);
        let top = area.y + area.height - card_h as u16;
        buf.set_stringn(
            area.x,
            top,
            "─".repeat(area.width as usize),
            area.width as usize,
            dim(),
        );
        for (i, (tag, text)) in card.iter().enumerate().take(card_h - 1) {
            buf.set_stringn(x, top + 1 + i as u16, text, width, tag_style(*tag));
        }
        height -= card_h;
    }
    if !d.loaded && d.lines().is_empty() {
        buf.set_stringn(x, area.y + 1, "reading the history…", width, dim());
        return;
    }
    // The newest rows that fit, after the scroll: wrap from the end only as
    // far as the pane needs.
    let want = height + d.scroll;
    let mut rows: Vec<(Tag, String)> = Vec::new();
    for (tag, text) in d.lines().iter().rev() {
        let mut wrapped = wrap(text, width);
        while let Some(r) = wrapped.pop() {
            rows.push((*tag, r));
        }
        if rows.len() >= want {
            break;
        }
    }
    // A scroll past the start stops at the start.
    let scroll = d.scroll.min(rows.len().saturating_sub(height));
    let shown: Vec<&(Tag, String)> = rows.iter().skip(scroll).take(height).collect();
    for (i, (tag, text)) in shown.into_iter().rev().enumerate() {
        buf.set_stringn(x, area.y + 1 + i as u16, text, width, tag_style(*tag));
    }
    if scroll > 0 {
        let more = format!(" ↓ {scroll} more ");
        let mx = area.x + area.width.saturating_sub(more.width() as u16);
        buf.set_stringn(
            mx,
            area.y + area.height - 1,
            &more,
            more.width(),
            dim().add_modifier(Modifier::REVERSED),
        );
    }
}

/// `ses …a1b2c3 · DM · glm-5.3-flash · ◐ turn 4`.
fn detail_header(app: &App, sid: &str) -> String {
    let mut parts = vec![format!("ses …{}", short(sid)), app.board.name(sid)];
    if let Some(m) = app.board.info(sid).and_then(|s| s.model.clone()) {
        parts.push(m);
    }
    if let Some(v) = app.board.view(sid) {
        parts.push(pill(&v.attention));
    }
    parts.join(" · ")
}

/// `text` cut into rows of at most `width` columns: after the last space
/// that fits, else at the width itself. An empty text is one empty row.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut row = String::new();
    let mut row_w = 0usize;
    // The row's last space: where it ends, in bytes and in columns.
    let mut space: Option<(usize, usize)> = None;
    for ch in text.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        // A space that does not fit ends the row where it is.
        if ch == ' ' && row_w + w > width {
            rows.push(std::mem::take(&mut row));
            row_w = 0;
            space = None;
            continue;
        }
        if row_w + w > width && !row.is_empty() {
            match space {
                // Break after the space: the word that did not fit moves on.
                Some((at, cols)) => {
                    let rest = row.split_off(at);
                    rows.push(row.trim_end().to_string());
                    row = rest;
                    row_w -= cols;
                }
                None => {
                    rows.push(std::mem::take(&mut row));
                    row_w = 0;
                }
            }
            space = None;
        }
        row.push(ch);
        row_w += w;
        if ch == ' ' {
            space = Some((row.len(), row_w));
        }
    }
    rows.push(row);
    rows
}

fn footer(app: &App, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
    let keys = "[?] help  [q] quit ";
    let typed = match &app.mode {
        Mode::Filter => Some(("/", app.filter.as_str())),
        Mode::Input => Some(("> ", app.input.as_str())),
        Mode::Note(_) => Some(("decline, with a note (enter: none)> ", app.note.as_str())),
        _ => None,
    };
    if let Some((prompt, text)) = typed {
        // The end of what is typed, when it is longer than the line.
        let room = (area.width as usize).saturating_sub(prompt.width() + 3);
        let line = format!(" {prompt}{}", tail_fit(text, room));
        buf.set_stringn(area.x, area.y, &line, area.width as usize, Style::default());
        let x = (line.width() as u16).min(area.width.saturating_sub(1));
        return Some((area.x + x, area.y));
    }
    if let Some(words) = app.armed_words() {
        buf.set_stringn(
            area.x + 1,
            area.y,
            &words,
            area.width.saturating_sub(1) as usize,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
        return None;
    }
    let room = (area.width as usize).saturating_sub(keys.width() + 1);
    if let Some((tag, text)) = &app.flash {
        buf.set_stringn(area.x + 1, area.y, text, room, tag_style(*tag));
    }
    let x = area.x + area.width.saturating_sub(keys.width() as u16);
    buf.set_stringn(x, area.y, keys, keys.width(), dim());
    None
}

/// The end of `text` that fits in `room` columns, with `…` before it when
/// it was cut.
fn tail_fit(text: &str, room: usize) -> String {
    if text.width() <= room {
        return text.to_string();
    }
    let mut kept: Vec<char> = Vec::new();
    let mut used = 1;
    for c in text.chars().rev() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > room {
            break;
        }
        used += w;
        kept.push(c);
    }
    kept.push('…');
    kept.into_iter().rev().collect()
}

/// The keys (design §2.9), over the body.
fn help(area: Rect, buf: &mut Buffer) {
    const KEYS: [&str; 16] = [
        " keys",
        " ↑ ↓  j k   move (on a narrow screen's session: scroll it)",
        " enter      open the session under the cursor",
        " tab ⇧tab   the next / previous that needs you, then done (◆)",
        " y t n      approve / approve + trust / decline (with a note)",
        " esc        back to the list (narrow screens); clear the filters",
        " i          type to the open session: enter sends, esc leaves",
        " s          stop the open conversation (s twice)",
        " c          cancel the open task (c twice)",
        " PgUp PgDn  scroll the open session (End: back to its end)",
        " / text     filter by text (enter keeps it, esc clears)",
        " b w r d a  only needs you, working, ready, done; all",
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
