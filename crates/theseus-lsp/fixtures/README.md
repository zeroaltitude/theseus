# Live fixtures

One small project per language for `tests/live.rs`: one planted type error (`count` given a string) and one
call across files (`total`, defined in `ledger`). The live tests copy a fixture to a scratch directory before a
server sees it, so nothing here is written to.
