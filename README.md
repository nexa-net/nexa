# NexaNet

**Simple distributed container orchestration.**

80% of Kubernetes use-cases with 20% of the complexity.

NexaNet orchestrates containers across machines with minimal configuration. No CRDs, no operators, no YAML explosion — just deploy and run.

## Quick Start

```bash
# Start the daemon
nexad

# Deploy a service
nexa deploy app.yaml

# Check status
nexa pods
nexa deployments

# Stream logs
nexa logs api

# Scale up
nexa scale api 5
```

## Deployment Spec

```yaml
project: ecommerce

deployment:
  name: api

replicas: 3
image: ghcr.io/company/api:latest

ports:
  - 3000

network:
  public: true
  domain: api.example.com
  https: true

env:
  DATABASE_URL: "postgres://localhost/db"

healthcheck:
  path: /health
  interval: 10s
```

## Core Concepts

| Concept    | Description                                    |
|------------|------------------------------------------------|
| Project    | Isolation boundary for deployments and networks |
| Deployment | A service definition with image, replicas, config |
| Pod        | A running container instance of a deployment    |
| Network    | Automatic per-project container networking      |
| Route      | Public exposure with optional TLS               |
| Secret     | Encrypted environment configuration             |
| Volume     | Persistent storage mount                        |

## Architecture

```
┌─────────┐       ┌─────────────────────────────────┐
│ nexa CLI │──────▶│            nexad                 │
└─────────┘  HTTP │  ┌───────────┐  ┌─────────────┐  │
                  │  │ API Layer │  │ Orchestrator │  │
                  │  └───────────┘  └──────┬──────┘  │
                  │                        │         │
                  │  ┌─────────────────────▼───────┐ │
                  │  │    Container Runtime         │ │
                  │  │  (Docker / containerd)       │ │
                  │  └─────────────────────────────┘ │
                  └─────────────────────────────────┘
```

- **nexad** — The daemon. Manages containers, networking, scheduling, and state.
- **nexa** — The CLI. Deploys, scales, inspects, and streams logs.
- **nexa-core** — Shared types, traits, and the container runtime abstraction.

## Building from Source

```bash
git clone https://github.com/nexa-net/nexa.git
cd nexa
cargo build --release
```

Binaries are placed in `target/release/`:
- `nexad` — the daemon
- `nexa` — the CLI

## Requirements

- Rust 1.85+
- Docker (running)

## Project Structure

```
crates/
  nexa-core/    # Shared types, models, runtime abstraction
  nexad/        # Daemon — orchestrator, API server
  nexa-cli/     # CLI client
examples/       # Example deployment specs
```

## Roadmap

- [x] Phase 1: Single-node deployments, pod lifecycle, CLI
- [ ] Phase 2: Multi-node clustering, scheduler, node management
- [ ] Phase 3: Automatic networking, routing, TLS
- [ ] Phase 4: Web dashboard

## Philosophy

NexaNet is intentionally opinionated and minimal. It targets developers, startups, self-hosters, and small teams who need container orchestration without enterprise complexity.

Every feature must justify its complexity. If it makes the system harder to understand or operate, it doesn't ship.

## License

Apache-2.0
