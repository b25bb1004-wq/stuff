# Monolith Deploy

Local multi-container deployment for Iridium, Argo Registry, and a 3-node Aurum cluster.

## Services and Ports

All ports bind exclusively to `127.0.0.1`:

- `iridium`: Iridium SQL database TCP server on `127.0.0.1:7400` (persisted in named volume `iridium-data` at `/data`).
- `registry`: Argo in-memory package registry HTTP server on `127.0.0.1:7500`.
- `aurum1`, `aurum2`, `aurum3`: 3-validator Aurum network peered over UDP inside the Compose network, exposing HTTP `/health` on `127.0.0.1:7601`, `127.0.0.1:7602`, and `127.0.0.1:7603`.

## Start and Stop

From the repository root:

```sh
docker compose -f crates/deploy/docker/compose.yaml up --build -d
```

Stop cleanly (SIGTERM, 10s grace period):

```sh
docker compose -f crates/deploy/docker/compose.yaml stop
```

## Health Checks

Check container health status:

```sh
docker compose -f crates/deploy/docker/compose.yaml ps
```

Query HTTP endpoints directly:

```sh
curl http://127.0.0.1:7500/index
curl http://127.0.0.1:7601/health
curl http://127.0.0.1:7602/health
curl http://127.0.0.1:7603/health
```

Each Aurum node returns `height=<n> tip=<hex> peers=<n>`.

## Security Note on Validator Keys

The validator keys and Xenon peer keys used by `aurum-node` are deterministic **public demo keys** derived from `sha256("monolith-demo-validator-" + i)` (with `1_000_000` genesis balance each). They are **not secret** and must only be used for local testing and demonstrations.
