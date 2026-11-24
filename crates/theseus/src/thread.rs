//! `theseus thread`: threads were retired on 2026-11-22 (issue 44), and no command reads their stored
//! records, which are kept. `thread import --vestige PATH` is a note import and moved to
//! `theseus import vestige PATH`; the old spelling still runs it, and says so.

use anyhow::Result;
use clap::Subcommand;
use theseus_client::Conn;

#[derive(Subcommand, Debug)]
pub enum ThreadCmd {
    /// Moved: `theseus import vestige PATH` imports a Vestige dump as notes.
    #[command(hide = true)]
    Import(crate::thread_import::Args),
}

pub async fn run(conn: &mut Conn, json: bool, cmd: ThreadCmd) -> Result<()> {
    match cmd {
        ThreadCmd::Import(a) => crate::thread_import::run(conn, json, a).await,
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    /// No `thread` verb reads the stored records any more: `list`, `show` and the bare verb do not
    /// parse, and the moved import's old spelling still does.
    #[test]
    fn no_thread_verb_reads_the_records_and_the_old_import_still_parses() {
        let parse = |args: &[&str]| crate::Cli::try_parse_from(args);
        for gone in [
            &["theseus", "thread"][..],
            &["theseus", "thread", "list"],
            &["theseus", "thread", "show", "tides"],
        ] {
            assert!(parse(gone).is_err(), "{gone:?} still parses");
        }
        assert!(parse(&["theseus", "thread", "import", "--vestige", "dump.json"]).is_ok());
    }
}
