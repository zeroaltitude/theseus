//! Health's `connections:` line (theseus-7vtp): the socket's connections held
//! against the daemon's ceiling, those refused at it, and the open-files limit
//! they sit under. Apart from `render.rs`, whose length the shape budget caps.

use theseus_protocol::ConnectionsHealth;

use super::{push, Line, Tag};

/// The line; loud (the bad tag) once any connection was refused. A daemon
/// from before it sends nothing, and says nothing.
pub(super) fn push_health(o: &mut Vec<Line>, c: &ConnectionsHealth) {
    if *c == ConnectionsHealth::default() {
        return;
    }
    let tag = if c.refused > 0 { Tag::Bad } else { Tag::Plain };
    push(o, tag, &connections_line(c));
}

/// `connections: 3 of 768 · 0 refused · open files 65536 of 1048576`. A
/// daemon that has set no ceiling says only what it holds (a `--stdio` one).
pub fn connections_line(c: &ConnectionsHealth) -> String {
    if c.ceiling == 0 || c.fd_soft == 0 {
        return format!("connections: {} held · {} refused", c.held, c.refused);
    }
    format!(
        "connections: {} of {} · {} refused · open files {} of {}",
        c.held, c.ceiling, c.refused, c.fd_soft, c.fd_hard
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_says_what_is_held_against_the_ceiling_and_the_limit() {
        let c = ConnectionsHealth {
            held: 3,
            ceiling: 768,
            refused: 0,
            fd_soft: 65536,
            fd_hard: 1_048_576,
        };
        assert_eq!(
            connections_line(&c),
            "connections: 3 of 768 · 0 refused · open files 65536 of 1048576"
        );
        let no_ceiling = ConnectionsHealth {
            held: 1,
            ..Default::default()
        };
        assert_eq!(
            connections_line(&no_ceiling),
            "connections: 1 held · 0 refused"
        );
        // A daemon before it sends the default, and health says nothing.
        let mut o = Vec::new();
        push_health(&mut o, &ConnectionsHealth::default());
        assert!(o.is_empty());
    }
}
