The service's throughput depends on its batch size. Benchmark the sizes 16, 32, 64 and 128 with `bench-batch SIZE`; each run takes a while.

Then write the size with the highest throughput to /app/bench.toml as the line `batch_size = SIZE`.
