A bug landed in one of the four commits listed in /app/commits.txt, oldest first; the oldest is good. Test the commits, each in a worktree of its own: `make-worktree COMMIT` makes one, and `test-commit COMMIT` runs the tests in it; both take a while.

Write the first commit whose tests fail to /app/first-bad.txt, alone.
