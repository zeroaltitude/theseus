//! The daemon's units cap glibc's arenas (theseus-mem-daemon-rest).

use super::layout::{system_unit, user_unit};
use super::USER_UNIT;

/// Both daemon units set `MALLOC_ARENA_MAX` once, in their `[Service]`, so only
/// the daemon takes it: a job and the index tender start with an environment
/// of their own.
#[test]
fn the_daemon_units_cap_the_allocators_arenas() {
    let exec = vec!["/opt/theseus/bin/theseusd".to_string()];
    let user = user_unit(USER_UNIT, &exec, &[]).unwrap();
    let system = system_unit(&exec).unwrap();
    for (name, unit) in [("user", &user), ("system", &system)] {
        let (_, service) = unit.split_once("[Service]").unwrap();
        let service = service.split("[Install]").next().unwrap();
        assert_eq!(
            service
                .matches("\nEnvironment=\"MALLOC_ARENA_MAX=2\"\n")
                .count(),
            1,
            "{name}:\n{unit}"
        );
    }
}
