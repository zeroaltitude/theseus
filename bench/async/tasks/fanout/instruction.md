Ingest the six parts 1, 2, 3, 4, 5 and 6, each with `ingest PART`; each takes a while. The upstream is flaky: a transient error says so, and the same part tried again works. A part ingested twice has its rows duplicated, so ingest each part successfully exactly once.

Then write /app/ingest-summary.txt: one line per part, the part and its row count (as `ingest` prints it) separated by a space, then a last line `total N`, N the sum of the six row counts.
