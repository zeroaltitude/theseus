Last week a change to `ratelimit/bucket.py` let one client send far more requests than its limit for about an hour,
and nothing in our test run noticed, because that module has no tests at all. Please make sure the next change that
breaks how the bucket refills or caps fails the suite.
