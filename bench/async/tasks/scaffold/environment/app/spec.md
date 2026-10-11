# Configuration spec

Each file goes under /app/config/ and is YAML: one `key: value` line per key, in any order.

## api.yaml

- service: api
- port: 8080
- replicas: 3

## worker.yaml

- service: worker
- queue: jobs
- replicas: 2

## scheduler.yaml

- service: scheduler
- interval_s: 60

## cache.yaml

- service: cache
- port: 6379
- max_mb: 512

## db.yaml

- service: db
- port: 5432
- pool: 20

## search.yaml

- service: search
- port: 9200
- shards: 4

## mail.yaml

- service: mail
- port: 2525
- from: noreply@example.test

## gateway.yaml

- service: gateway
- port: 443
- upstream: api:8080
