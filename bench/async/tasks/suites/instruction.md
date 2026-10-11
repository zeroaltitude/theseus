The packages auth, billing, catalog and search each have a test suite: `run-suite PACKAGE` runs one, and each takes a while.

Find every failing test and write them to /app/failing.txt, one per line, as `run-suite` names them (PACKAGE::TEST).
