"""A token bucket: `capacity` tokens at most, refilled at `rate` tokens a second."""


class TokenBucket:
    def __init__(self, capacity: float, rate: float, now: float = 0.0) -> None:
        if capacity <= 0 or rate < 0:
            raise ValueError("capacity must be positive and rate not negative")
        self.capacity = float(capacity)
        self.rate = float(rate)
        self.tokens = float(capacity)
        self.stamp = now

    def _refill(self, now: float) -> None:
        if now > self.stamp:
            self.tokens = min(self.capacity, self.tokens + (now - self.stamp) * self.rate)
            self.stamp = now

    def take(self, n: float = 1.0, now: float = 0.0) -> bool:
        """Take `n` tokens at time `now`; False, taking nothing, when there are not enough."""
        self._refill(now)
        if n > self.tokens:
            return False
        self.tokens -= n
        return True

    def wait_for(self, n: float = 1.0, now: float = 0.0) -> float:
        """Seconds until `n` tokens could be taken (0 when they can now)."""
        self._refill(now)
        if n <= self.tokens:
            return 0.0
        if self.rate == 0:
            return float("inf")
        return (n - self.tokens) / self.rate
