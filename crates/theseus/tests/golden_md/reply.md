# Port Wenlow tides

A **small library** for an invented harbour's tide table. It reads `HH:MM height` lines, finds the high and low waters between their neighbours, and works out the tidal range between the highest and the lowest water of the day, which the harbour board shows in metres.

## Its checks

- one *unit test* for each function, against a single invented day of readings
- a slow survey of thirty stations that prints its progress as it goes
