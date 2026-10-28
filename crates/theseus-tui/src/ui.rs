//! Drawing (design `stage2` §2.9's layout): the header, the sidebar with its
//! trees, the detail pane, and the footer with the input line, into a ratatui
//! buffer. Nothing here changes the app: the same app draws the same buffer,
//! so tests snapshot it.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use theseus_client::render::diffview::{MARK_OFF, MARK_ON};
use theseus_client::render::{glyph, pill, Tag};
use theseus_protocol::Level;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, Link, Mode};
use crate::board::{short, Only, Row};
use crate::card;
use crate::standing::Standing;
use crate::styled::{self, PaneRow};
use crate::work::Form;

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
    let side = if app.board.work.live {
        work_sidebar_width(area.width)
    } else {
        sidebar_width(area.width)
    };
    if app.mode == Mode::SelfView {
        // The self view takes the whole body (theseus-pw1q.19).
        self_view(app, body, buf);
    } else if app.wide() {
        sidebar(app, Rect::new(0, body.y, side, body.height), buf);
        for y in body.y..body.y + body.height {
            buf.set_string(side, y, "│", dim());
        }
        let right = Rect::new(side + 1, body.y, area.width - side - 1, body.height);
        detail(app, right, buf);
    } else if app.shown().is_some() || app.follow.is_some() {
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
    if app.mode == Mode::Threads {
        crate::threads::draw_picker(app, body, buf);
    }
    if app.mode == Mode::Standing {
        let lines = app
            .standing
            .as_ref()
            .map(Standing::lines)
            .unwrap_or_default();
        overlay(&lines, body, buf);
    }
    cursor
}

/// The sidebar's width beside the detail pane: three tenths of the screen,
/// from 36 to 56 columns.
pub fn sidebar_width(width: u16) -> u16 {
    (width * 3 / 10).clamp(36, 56)
}

/// With the work board, a row has more to say: two fifths of the screen, from
/// 36 to 64 columns (theseus-k2vca).
pub fn work_sidebar_width(width: u16) -> u16 {
    (width * 2 / 5).clamp(36, 64)
}

/// The self view: its lines, as many as fit.
fn self_view(app: &App, area: Rect, buf: &mut Buffer) {
    for (i, (tag, line)) in app
        .self_lines(area.width)
        .iter()
        .enumerate()
        .take(area.height as usize)
    {
        let style = if i == 0 {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            tag_style(*tag)
        };
        buf.set_stringn(area.x, area.y + i as u16, line, area.width as usize, style);
    }
}

fn header(app: &App, area: Rect, buf: &mut Buffer) {
    let mut t = app.board.tally(&|v| app.is_done(v));
    // A seal waiting on the owner is a need the executions' count lacks (theseus-5i8i).
    t.needs += app.board_threads.seals_pending();
    let mut left = String::from(" theseus");
    // `● 1 needs you · ◐ 3 working · ✗ 1 failed · ✓ 2 done` (theseus-k2vca).
    for (n, text) in [
        (
            t.needs,
            format!(
                "● {} need{} you",
                t.needs,
                if t.needs == 1 { "s" } else { "" }
            ),
        ),
        (t.working, format!("◐ {} working", t.working)),
        (t.failed, format!("✗ {} failed", t.failed)),
        (t.done, format!("✓ {} done", t.done)),
    ] {
        if n > 0 {
            left.push_str(&format!(" · {text}"));
        }
    }
    if t.needs + t.working + t.failed + t.done == 0 {
        let n = app.board.len();
        left.push_str(&format!(" · {n} session{}", if n == 1 { "" } else { "s" }));
    }
    if let Some(w) = &app.sync_warn {
        left.push_str(&format!(" · ⚠ {w}"));
    }
    if let Some(p) = app.standing_pill() {
        left.push_str(&format!(" · {p}"));
    }
    // The daemon's own work, the root collapsed (theseus-jn17m).
    let system: Vec<_> = app.system.values().cloned().collect();
    if let Some(s) = theseus_protocol::work_system::collapsed(&system) {
        left.push_str(&format!(" · {s}"));
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
    // The current thread, in the status line (theseus-j0yh): within half the width.
    let status = app.thread_status(area.width as usize / 2);
    let right = match status {
        Some(t) => format!("{t} · {} · {link} ", (app.hm)(app.now_ms)),
        None => format!("{} · {link} ", (app.hm)(app.now_ms)),
    };
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

/// The Threads block (theseus-5i8i) at the head of the sidebar: a group for
/// each live thread, folded or open, over a rule. At most half the sidebar,
/// scrolled to the cursor while the block has the keys. Returns the rows it
/// took; nothing when no thread is on the board.
fn threads_block(app: &App, area: Rect, buf: &mut Buffer) -> u16 {
    let lines = app.board_threads.lines(app.now_ms, area.width as usize);
    if lines.is_empty() || area.height < 6 {
        return 0;
    }
    let room = (area.height / 2).max(2) as usize;
    let top = match app.mode {
        Mode::BoardThreads if app.board_threads.cursor >= room => {
            app.board_threads.cursor + 1 - room
        }
        _ => 0,
    };
    let shown = lines.len().min(room);
    for (i, line) in lines.iter().enumerate().skip(top).take(room) {
        let y = area.y + (i - top) as u16;
        let selected = app.mode == Mode::BoardThreads && i == app.board_threads.cursor;
        let base = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        let row = Rect::new(area.x, y, area.width, 1);
        buf.set_style(row, base);
        let style = match line.kind {
            crate::board_threads::Kind::Header { .. } => {
                level_style(line.level, false).add_modifier(Modifier::BOLD)
            }
            _ => level_style(line.level, false),
        };
        buf.set_stringn(
            area.x + 1,
            y,
            &line.text,
            area.width.saturating_sub(1) as usize,
            base.patch(style),
        );
    }
    let rule = area.y + shown as u16;
    buf.set_stringn(
        area.x,
        rule,
        "─".repeat(area.width as usize),
        area.width as usize,
        dim(),
    );
    shown as u16 + 1
}

fn sidebar(app: &App, area: Rect, buf: &mut Buffer) {
    let used = threads_block(app, area, buf);
    let area = Rect::new(area.x, area.y + used, area.width, area.height - used);
    // The future sits under the task tree (theseus-nuhsz).
    let below = crate::horizon::rows_for(app, area.height);
    if below > 0 && area.height > below + 1 {
        let tree = Rect::new(area.x, area.y, area.width, area.height - below);
        crate::horizon::rule(Rect::new(area.x, tree.y + tree.height, area.width, 1), buf);
        let section = Rect::new(area.x, tree.y + tree.height + 1, area.width, below - 1);
        tree_rows(app, tree, buf);
        crate::horizon::draw(app, section, buf);
        return;
    }
    tree_rows(app, area, buf);
}

fn tree_rows(app: &App, area: Rect, buf: &mut Buffer) {
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
    // A row is a line, or two when a finished task's report shows under it.
    let lines: Vec<(usize, bool)> = rows
        .iter()
        .enumerate()
        .flat_map(|(i, r)| {
            let under = r.facts.as_ref().is_some_and(|f| f.report.is_some());
            std::iter::once((i, false)).chain(under.then_some((i, true)))
        })
        .collect();
    // Keep the cursor's row on screen: scroll only as far as it needs.
    let height = area.height as usize;
    let top = match cursor.and_then(|c| lines.iter().rposition(|(i, _)| *i == c)) {
        Some(l) if l >= height => l + 1 - height,
        _ => 0,
    };
    for (n, (i, under)) in lines.iter().copied().enumerate().skip(top).take(height) {
        let (row, y) = (&rows[i], area.y + (n - top) as u16);
        let line = Rect::new(area.x, y, area.width, 1);
        if under {
            draw_report(row, line, buf);
        } else {
            draw_row(app, row, line, buf, Some(i) == cursor);
        }
    }
}

/// A finished task's first report line, under its row: `◆` while it is new.
fn draw_report(row: &Row, area: Rect, buf: &mut Buffer) {
    let Some(report) = row.facts.as_ref().and_then(|f| f.report.as_deref()) else {
        return;
    };
    let lead = format!(
        " {}{}",
        "  ".repeat(row.depth + usize::from(row.depth > 0)),
        if row.done { DONE } else { ' ' }
    );
    let (x, _) = buf.set_stringn(
        area.x,
        area.y,
        &lead,
        area.width as usize,
        level_style(row.level, row.done),
    );
    let room = (area.width as usize).saturating_sub((x - area.x) as usize + 1);
    buf.set_stringn(x + 1, area.y, crate::work::cut(report, room), room, dim());
}

/// One row: ` ● confirm proc.run  DM        $0.42`, a task indented under its
/// parent (`└ ◐ turn 2  task b4c5d6`).
fn draw_row(app: &App, row: &Row, area: Rect, buf: &mut Buffer, selected: bool) {
    let mut prefix = String::from(" ");
    if row.depth > 0 {
        prefix.push_str(&"  ".repeat(row.depth));
        prefix.push(if row.last { '└' } else { '├' });
        prefix.push(' ');
    }
    let mark = match row.facts.as_ref().map(|f| &f.form) {
        _ if row.dismissed => '✗',
        Some(Form::Failed { .. }) => '✗',
        Some(Form::Done { .. }) => '✓',
        _ if row.done => DONE,
        _ => glyph(row.level),
    };
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
        if row.dismissed {
            dim()
        } else {
            level_style(row.level, row.done)
        },
        &mut x,
        &mut used,
    );
    if let Some(facts) = &row.facts {
        // What the work board says: the title, then what it is doing and
        // for how long. The title is cut first, then the doing, and the cost
        // goes before either (theseus-k2vca).
        let mut doing = crate::work::doing(facts, app.now_ms, &app.hm);
        if row.dismissed {
            doing.push_str(" · dismissed");
        }
        let suffix = if row.folded > 0 {
            format!(" +{}", row.folded)
        } else {
            String::new()
        };
        let left = width.saturating_sub((x - area.x) as usize);
        let cost_room = left.saturating_sub(cost.width());
        let whole = row.name.width() + suffix.width() + 2 + doing.width();
        let (room, cost_shown) = if whole <= cost_room {
            (cost_room, true)
        } else {
            (left, false)
        };
        let (title, doing) = crate::work::fit(&row.name, &suffix, &doing, room);
        let (mut x, mut used) = (x, 0usize);
        let put = |text: &str, style: Style, x: &mut u16, used: &mut usize, buf: &mut Buffer| {
            let (nx, _) = buf.set_stringn(
                *x,
                area.y,
                text,
                room.saturating_sub(*used),
                base.patch(style),
            );
            *used += (nx - *x) as usize;
            *x = nx;
        };
        put(&title, Style::default(), &mut x, &mut used, buf);
        put("  ", Style::default(), &mut x, &mut used, buf);
        let style = if row.level == Level::Idle && !row.done {
            dim()
        } else {
            Style::default()
        };
        put(&doing, style, &mut x, &mut used, buf);
        if cost_shown {
            let cx = area.x + area.width.saturating_sub(cost.width() as u16);
            buf.set_stringn(cx, area.y, &cost, cost.width(), base.patch(dim()));
        }
        return;
    }
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
    if let Some(f) = &app.follow {
        return follow_pane(app, f, area, buf);
    }
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
    // rule (design §2.9, "Answering"), each line wrapped at the pane's edge.
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
        // No question: a failure the owner has not dismissed (theseus-7zgsm).
        .or_else(|| {
            app.failure().map(|v| {
                card::failure_lines(v, app.refused.get(&v.execution_id).map(String::as_str))
            })
        })
        .unwrap_or_default()
        .into_iter()
        .flat_map(|(tag, text)| card_rows(&text, width).into_iter().map(move |r| (tag, r)))
        .collect();
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
            put(buf, x, top + 1 + i as u16, text, width, *tag);
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
    let rows = styled::rows_from_end(d.lines(), width, want);
    // A scroll past the start stops at the start.
    let scroll = d.scroll.min(rows.len().saturating_sub(height));
    let shown: Vec<&PaneRow> = rows.iter().skip(scroll).take(height).collect();
    for (i, row) in shown.into_iter().rev().enumerate() {
        let y = area.y + 1 + i as u16;
        match row {
            PaneRow::Plain(tag, text) => {
                put(buf, x, y, text, width, *tag);
            }
            PaneRow::Drawn(spans) => {
                let mut at = x;
                for (text, style) in spans {
                    let room = (x as usize + width).saturating_sub(at as usize);
                    (at, _) = buf.set_stringn(at, y, text, room, *style);
                }
            }
        }
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

/// A running job's output (`l`): its time against its usual, then the newest
/// rows that fit, wrapped at the pane's edge.
fn follow_pane(app: &App, f: &crate::follow::Follow, area: Rect, buf: &mut Buffer) {
    let width = area.width.saturating_sub(1) as usize;
    let x = area.x + 1;
    buf.set_stringn(
        x,
        area.y,
        f.header(app.now_ms, &app.board.name(&f.session_id)),
        width,
        Style::default().add_modifier(Modifier::BOLD),
    );
    if f.job.is_none() {
        buf.set_stringn(x, area.y + 1, "looking for its job…", width, dim());
        return;
    }
    let height = area.height.saturating_sub(1) as usize;
    let mut rows: Vec<String> = Vec::new();
    for line in f.lines().into_iter().rev() {
        let mut wrapped = wrap(line, width);
        while let Some(r) = wrapped.pop() {
            rows.push(r);
        }
        if rows.len() >= height {
            break;
        }
    }
    for (i, r) in rows.iter().take(height).rev().enumerate() {
        buf.set_stringn(x, area.y + 1 + i as u16, r, width, Style::default());
    }
}

/// `ses …a1b2c3 · DM · glm-5.3-flash · ◐ turn 4`.
fn detail_header(app: &App, sid: &str) -> String {
    // A task says whose it is and the way back (theseus-mu0e6).
    let mut parts = match app.breadcrumb(sid) {
        Some(crumb) => vec![crumb, app.board.name(sid)],
        None => vec![format!("ses …{}", short(sid)), app.board.name(sid)],
    };
    if let Some(m) = app.board.info(sid).and_then(|s| s.model.clone()) {
        parts.push(m);
    }
    if let Some(v) = app.board.view(sid) {
        parts.push(pill(&v.attention));
    }
    // A thread's session says so (theseus-j0yh): the reply's tag, dim.
    if let Some(t) = app.thread_of(sid) {
        parts.push(theseus_client::render::thread::reply_tag(&t.thread.title));
    }
    // The mode (theseus-lyxy), and the plan's count while there is one.
    if let Some(d) = app.detail.as_ref().filter(|d| d.session_id == sid) {
        parts.push(d.mode.status());
        parts.extend(theseus_protocol::modes::plan_line(&d.plan));
    }
    parts.join(" · ")
}

/// The most rows a card line takes.
const CARD_ROWS: usize = 5;

/// A card line wrapped at the pane's edge, its later rows as indented as the
/// line: a long reason is read, where one row cut it (theseus-94a6: a shared
/// place's clause ends a private fetch's reason). Past `CARD_ROWS` rows its
/// middle goes: the first row, `…`, and the last rows, which say why.
pub fn card_rows(text: &str, width: usize) -> Vec<String> {
    let body = text.trim_start_matches(' ');
    let lead = &text[..text.len() - body.len()];
    let mut rows = wrap(body, width.saturating_sub(lead.len()));
    if rows.len() > CARD_ROWS {
        let last = rows.split_off(rows.len() - (CARD_ROWS - 2));
        rows.truncate(1);
        rows.push("…".into());
        rows.extend(last);
    }
    rows.into_iter().map(|r| format!("{lead}{r}")).collect()
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
    carry_marks(&mut rows);
    rows
}

/// A marked word that a wrap cut keeps its mark on the next row: each row
/// closes what it opened, and the next opens it again.
fn carry_marks(rows: &mut [String]) {
    let mut open = false;
    for r in rows.iter_mut() {
        let was = open;
        for c in r.chars() {
            match c {
                MARK_ON => open = true,
                MARK_OFF => open = false,
                _ => {}
            }
        }
        if was {
            r.insert(0, MARK_ON);
        }
        if open {
            r.push(MARK_OFF);
        }
    }
}

/// `text` at `x, y` in its tag's style; a diff's changed words (between
/// `MARK_ON` and `MARK_OFF`) are reversed in the line's colour, bold.
pub(crate) fn put(buf: &mut Buffer, x: u16, y: u16, text: &str, width: usize, tag: Tag) {
    let base = tag_style(tag);
    let text = &*crate::glyph::tidy(text);
    if !text.contains(MARK_ON) {
        buf.set_stringn(x, y, text.replace(MARK_OFF, ""), width, base);
        return;
    }
    let mark = match tag {
        Tag::Add => Style::default()
            .fg(Color::White)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD),
        _ => Style::default()
            .fg(Color::White)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD),
    };
    let (mut at, mut left, mut on) = (x, width, false);
    let mut seg = String::new();
    let mut flush = |seg: &mut String, on: bool, at: &mut u16, left: &mut usize| {
        if seg.is_empty() {
            return;
        }
        let (nx, _) = buf.set_stringn(*at, y, seg.as_str(), *left, if on { mark } else { base });
        *left = left.saturating_sub((nx - *at) as usize);
        *at = nx;
        seg.clear();
    };
    for c in text.chars() {
        match c {
            MARK_ON => {
                flush(&mut seg, on, &mut at, &mut left);
                on = true;
            }
            MARK_OFF => {
                flush(&mut seg, on, &mut at, &mut left);
                on = false;
            }
            _ => seg.push(c),
        }
    }
    flush(&mut seg, on, &mut at, &mut left);
}

fn footer(app: &App, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
    let keys = match app.mode {
        Mode::SelfView => app.self_keys(),
        _ => "[l] self  [?] help  [q] quit ",
    };
    let prompt = app.input_prompt();
    let new_task = match &app.mode {
        Mode::NewTask(home) => app.new_task_words(home.as_deref()),
        _ => String::new(),
    };
    let typed = match &app.mode {
        Mode::Filter => Some(("/", app.filter.as_str())),
        Mode::Input => Some((prompt.as_str(), app.input.as_str())),
        Mode::Note(_) => Some(("decline, with a note (enter: none)> ", app.note.as_str())),
        Mode::NewTask(_) => Some((new_task.as_str(), app.brief.as_str())),
        _ => None,
    };
    if let Some((prompt, text)) = typed {
        // The end of what is typed, when it is longer than the line; a
        // pasted line break shows as `↵` (theseus-8hcg).
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
    } else if app.mode == Mode::Horizon {
        let hint = "horizon: j/k move · z snooze · p pause/resume · r run now · x cancel · enter open · esc back";
        buf.set_stringn(area.x + 1, area.y, hint, room, dim());
    }
    let x = area.x + area.width.saturating_sub(keys.width() as u16);
    buf.set_stringn(x, area.y, keys, keys.width(), dim());
    None
}

/// The end of `text` that fits in `room` columns, with `…` before it when
/// it was cut; a line break shows as `↵` and a tab as a space. It reads only
/// the end it keeps, so a pasted megabyte costs a frame no more than a line
/// (theseus-8hcg's review).
fn tail_fit(text: &str, room: usize) -> String {
    let mut kept: Vec<char> = Vec::new();
    let mut used = 0;
    let mut cut = false;
    for c in text.chars().rev() {
        let c = match c {
            '\n' => '↵',
            '\t' => ' ',
            c => c,
        };
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > room {
            cut = true;
            break;
        }
        used += w;
        kept.push(c);
    }
    if cut {
        // Room for the `…`: the first kept go.
        while used + 1 > room {
            let Some(c) = kept.pop() else { break };
            used -= UnicodeWidthChar::width(c).unwrap_or(0);
        }
        kept.push('…');
    }
    kept.into_iter().rev().collect()
}

/// The keys (design §2.9), over the body.
fn help(area: Rect, buf: &mut Buffer) {
    const KEYS: [&str; 28] = [
        " keys",
        " ↑ ↓  j k   move (on a narrow screen's session: scroll it)",
        " enter      open the session under the cursor",
        " tab ⇧tab   the next / previous that needs you, then done (◆)",
        " y n        approve / decline (with a note)",
        " esc        back to the list (narrow screens); clear the filters",
        " i          type to the open session: enter sends, esc leaves, ⇧tab its mode",
        " s          stop the open conversation (s twice)",
        " S          stop it and cancel every task under it (S or y to confirm)",
        " c          cancel the open task (c twice)",
        " N          start a task (no model turn) under the conversation under the cursor",
        " P          pause the task under the cursor, or resume it if it is paused (no model turn)",
        " l          follow the running job of the session under the cursor (esc closes)",
        " PgUp PgDn  scroll the open session (End: back to its end)",
        " / text     filter by text (enter keeps it, esc clears)",
        " b w r d a  only needs you, working, ready, done; all",
        " ctrl-t     the threads: pick one (enter), pause p, resume r, done d",
        " /t         in the input line: the threads; /t N or /t NAME picks one",
        " T          the threads on the board: enter folds or opens; esc leaves",
        " r d o      on a failed row: retry (a task) / dismiss / open it",
        " L          what Theseus changed about itself (esc closes)",
        " p          the standing records that hold in the open session; in a task, back to its parent (⌫ too)",
        " I          the open session's newest turn in the inspector: its link",
        " H          the Horizon (reminders, routines, watches): j k move, z snooze, p pause, r run now, x cancel",
        " ?          this help",
        " q          quit (ctrl-c from anywhere)",
        "",
        " any key closes this",
    ];
    overlay(&KEYS, area, buf);
}

/// Lines in a box over the body, the first one bold; the same look as the
/// keys.
fn overlay<S: AsRef<str>>(lines: &[S], area: Rect, buf: &mut Buffer) {
    let w = lines.iter().map(|l| l.as_ref().width()).max().unwrap_or(0) as u16 + 2;
    let h = lines.len() as u16;
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let rect = Rect::new(x, y, w.min(area.width), h.min(area.height));
    buf.set_style(rect, Style::default().bg(Color::Black));
    for (i, l) in lines.iter().enumerate().take(rect.height as usize) {
        let l = l.as_ref();
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
        Tag::Bad | Tag::Del => Style::default().fg(Color::Red),
        Tag::Add => Style::default().fg(Color::Green),
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
