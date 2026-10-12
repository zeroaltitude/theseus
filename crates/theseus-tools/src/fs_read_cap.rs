//! `fs.read`'s own cap (theseus-v73m; the owner's default D6): a read
//! returns at most `FS_READ_MAX_CHARS` characters, about Claude Code's, and
//! never a hole. Its window stops at the last whole line that fits, with the
//! footer that names the next offset, so the text is contiguous and the
//! next call is plain. The runtime cuts an `fs_read` result the same way,
//! at the same cap (`Tool::result_max_chars`), should one pass it still
//! (a scrubbed secret's stand-in is longer than the secret).

/// The most characters of one `fs_read` result.
pub const FS_READ_MAX_CHARS: usize = 100_000;

/// What a window leaves for its footer and a stand-in's growth.
const FOOTER_ROOM: usize = 400;

/// What a window's rows may still take: its bytes (`[tools]
/// max_read_bytes`) and its characters (`FS_READ_MAX_CHARS`, less the
/// footer's room).
pub(crate) struct Room {
    bytes: usize,
    chars: usize,
}

impl Room {
    pub(crate) fn new(max_bytes: usize) -> Self {
        Self {
            bytes: max_bytes,
            chars: FS_READ_MAX_CHARS - FOOTER_ROOM,
        }
    }

    /// Whether `row` fits; if it does, it is taken.
    pub(crate) fn takes(&mut self, row: &str) -> bool {
        let chars = row.chars().count();
        if row.len() > self.bytes || chars > self.chars {
            return false;
        }
        self.bytes -= row.len();
        self.chars -= chars;
        true
    }
}
