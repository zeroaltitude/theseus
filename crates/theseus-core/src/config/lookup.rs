//! Where the config comes from (theseus-5aqz). The order: `--config`, then
//! `THESEUS_CONFIG` (clap reads both into one value, the flag first), then
//! the operator's own file, `~/.theseus/theseus.toml`, if it exists, then
//! the machine's, `/etc/theseus/theseus.toml`, if it exists.
//! `scripts/setup.sh` writes the machine's from the template, every secret
//! an `op://` reference. No default names anyone's vault (theseus-8d1b): a
//! deployment kept in 1Password names its note in its environment or its
//! service unit.

use std::path::Path;

/// The operator's own config file, the lookup's first default: read when
/// nothing names a config and it exists.
pub const DEFAULT_CONFIG: &str = "~/.theseus/theseus.toml";

/// The machine's config file, the lookup's second default: read when nothing
/// names a config and the operator has no file of their own.
pub const SYSTEM_CONFIG: &str = "/etc/theseus/theseus.toml";

/// What a start says when nothing names a config and neither file exists.
pub const NO_CONFIG: &str = "no config: set THESEUS_CONFIG (or --config) to your config's \
     op:// reference or file, or write one at ~/.theseus/theseus.toml or at \
     /etc/theseus/theseus.toml, read in that order (`theseusd example-config` prints a \
     template, and scripts/setup.sh writes the second from it)";

/// Where this process's config comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// `--config` or `THESEUS_CONFIG` named it: a file, or an `op://` reference.
    Named(String),
    /// Nothing named one, and the operator's own file exists.
    User,
    /// Nothing named one, the operator has no file of their own, and the
    /// machine's exists, at this path.
    System(String),
    /// Nothing named one, and neither file exists.
    Missing,
}

impl Lookup {
    /// The lookup: `named` is what `--config` or `THESEUS_CONFIG` gave,
    /// `system` the machine's file, and `there` whether a path is there.
    pub fn find(named: Option<&str>, system: &str, there: impl Fn(&Path) -> bool) -> Self {
        if let Some(n) = named.filter(|n| !n.is_empty()) {
            Lookup::Named(n.to_string())
        } else if there(&super::expand(DEFAULT_CONFIG)) {
            Lookup::User
        } else if there(Path::new(system)) {
            Lookup::System(system.to_string())
        } else {
            Lookup::Missing
        }
    }

    /// The source to read: what was named, or the file the lookup found.
    /// None when nothing was.
    pub fn source(&self) -> Option<&str> {
        match self {
            Lookup::Named(s) | Lookup::System(s) => Some(s),
            Lookup::User => Some(DEFAULT_CONFIG),
            Lookup::Missing => None,
        }
    }

    /// The source a unit names (`theseusd install --user`): the one found,
    /// else the operator's own file, where `NO_CONFIG` says to write one.
    pub fn or_default(&self) -> &str {
        self.source().unwrap_or(DEFAULT_CONFIG)
    }

    /// How the lookup found the source, in words that follow it: nothing
    /// when it was named.
    pub fn how(&self) -> &'static str {
        match self {
            Lookup::Named(_) | Lookup::Missing => "",
            Lookup::User => " (the default: nothing named a config)",
            Lookup::System(_) => {
                " (the default: nothing named a config, and there is no ~/.theseus/theseus.toml)"
            }
        }
    }
}

/// This process's lookup, `named` being clap's `--config` or
/// `THESEUS_CONFIG`. A path that cannot be examined (in a directory this user
/// cannot enter) counts as there, so its read fails saying why, rather than
/// the lookup passing it by. In a debug build, `THESEUS_TEST_SYSTEM_CONFIG`
/// stands in for the machine's file, so a test never reads the one on the
/// machine it runs on; a release build has no such stand-in.
pub fn find_config(named: Option<&str>) -> Lookup {
    Lookup::find(named, &system_config(), there)
}

/// Whether the lookup takes `p`: anything but a path that is not there.
fn there(p: &Path) -> bool {
    match std::fs::metadata(p) {
        Ok(_) => true,
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
}

fn system_config() -> String {
    #[cfg(debug_assertions)]
    if let Some(p) = std::env::var_os("THESEUS_TEST_SYSTEM_CONFIG") {
        return p.to_string_lossy().into_owned();
    }
    SYSTEM_CONFIG.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lookup on a machine where `files` are there.
    fn on(named: Option<&str>, files: &[&str]) -> Lookup {
        Lookup::find(named, "/etc/theseus/theseus.toml", |p| {
            files.iter().any(|f| p.ends_with(f))
        })
    }

    const OWN: &str = ".theseus/theseus.toml";
    const ETC: &str = "/etc/theseus/theseus.toml";

    /// Step 1 (`--config`) and step 2 (`THESEUS_CONFIG`) reach the lookup as
    /// one named value, which wins over both files; the binary's tests hold
    /// the flag over the variable (`tests/default_config.rs`).
    #[test]
    fn a_named_config_wins_over_both_files() {
        let l = on(Some("/srv/x.toml"), &[OWN, ETC]);
        assert_eq!(l, Lookup::Named("/srv/x.toml".into()));
        assert_eq!((l.source(), l.how()), (Some("/srv/x.toml"), ""));
        let vault = on(Some("op://<vault>/<item>/notesPlain"), &[]);
        assert_eq!(vault.source(), Some("op://<vault>/<item>/notesPlain"));
        // An empty value names nothing.
        assert_eq!(on(Some(""), &[ETC]), Lookup::System(ETC.into()));
    }

    /// Step 3: the operator's own file, when it is there, before the machine's.
    #[test]
    fn the_operators_own_file_comes_before_the_machines() {
        let l = on(None, &[OWN, ETC]);
        assert_eq!(l, Lookup::User);
        assert_eq!(l.source(), Some(DEFAULT_CONFIG));
        assert!(l.how().contains("nothing named a config"), "{}", l.how());
        assert_eq!(on(None, &[OWN]), Lookup::User);
    }

    /// Step 4: the machine's file, when the operator has none of their own.
    #[test]
    fn the_machines_file_when_the_operator_has_none() {
        let l = on(None, &[ETC]);
        assert_eq!(l, Lookup::System(ETC.into()));
        assert_eq!((l.source(), l.or_default()), (Some(ETC), ETC));
        assert!(
            l.how().contains("no ~/.theseus/theseus.toml"),
            "{}",
            l.how()
        );
    }

    /// Neither file: nothing to read, and a unit would name the operator's
    /// own file, where `NO_CONFIG` says to write one.
    #[test]
    fn neither_file_is_missing_and_says_where_one_goes() {
        let l = on(None, &[]);
        assert_eq!(l, Lookup::Missing);
        assert_eq!((l.source(), l.or_default()), (None, DEFAULT_CONFIG));
        for place in [DEFAULT_CONFIG, SYSTEM_CONFIG] {
            assert!(NO_CONFIG.contains(place), "{NO_CONFIG}");
        }
    }

    /// The real lookup passes a missing path by, but not one it cannot
    /// examine: that one is read, and its read says why it fails.
    #[test]
    fn a_path_that_cannot_be_examined_counts_as_there() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("theseus.toml");
        std::fs::write(&file, "").unwrap();
        assert!(there(&file));
        assert!(!there(&tmp.path().join("gone.toml")));
        let locked = tmp.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let examined = std::fs::metadata(locked.join("theseus.toml"));
        let inside = there(&locked.join("theseus.toml"));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Root can examine anything, and then finds the file is not there.
        match examined {
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => assert!(inside),
            _ => assert!(!inside),
        }
    }
}
