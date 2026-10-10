Check the health of the services auth, billing, search and mail with `health SERVICE`; each check takes a while.

Restart the one that is failing with `restart SERVICE`, check it again to verify it is healthy, and write its name alone to /app/restarted.txt. Leave the healthy services alone.
