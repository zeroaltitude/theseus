"""Survey every station of an invented coast, slowly: one station every 3
seconds, 30 stations, so it runs for 90 seconds and prints its progress."""

import sys
import time

STATIONS = 30
SECONDS_EACH = 3.0


def main():
    each = float(sys.argv[1]) if len(sys.argv) > 1 else SECONDS_EACH
    for n in range(1, STATIONS + 1):
        time.sleep(each)
        print(f"survey: station {n}/{STATIONS} read", flush=True)
    print("survey complete: 30 stations", flush=True)


if __name__ == "__main__":
    main()
