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

## Repositories

NexaNet is organized as a multi-repo project under the [`nexa-net`](https://github.com/nexa-net) GitHub organization:

| Repository | Description | Status |
|------------|-------------|--------|
| [`nexa-core`](https://github.com/nexa-net/nexa-core) | Shared types, domain models, port traits | Active |
| [`nexad`](https://github.com/nexa-net/nexad) | Daemon — orchestrator, API, adapters | Active |
| [`nexa-cli`](https://github.com/nexa-net/nexa-cli) | CLI client (`nexa` binary) | Active |
| [`nexa-proxy`](https://github.com/nexa-net/nexa-proxy) | Reverse proxy sidecar with auto TLS | Planned |
| [`nexa`](https://github.com/nexa-net/nexa) | This repo — specs, plans, project docs | Active |

### Hexagonal Architecture

```
nexa-core (domain + ports)
  ├── domain/         Pure business logic
  │   ├── orchestrator.rs
  │   ├── scheduler.rs
  │   ├── health.rs
  │   ├── restart.rs
  │   └── models/
  └── ports/          Trait definitions only
      ├── runtime.rs  (ContainerRuntime)
      ├── state.rs    (StateStore)
      ├── secrets.rs  (SecretStore)
      ├── proxy.rs    (ProxyBackend)
      ├── dns.rs      (DnsProvider)
      └── cluster.rs  (ClusterTransport)

nexad (adapters + composition root)
  └── adapters/
      ├── runtime/    Docker, containerd
      ├── state/      SQLite
      ├── secrets/    AES-256-GCM encrypted
      ├── proxy/      nexa-proxy, Caddy, Traefik, Nginx
      ├── dns/        hickory-dns
      └── cluster/    gRPC, local
```

## Building from Source

```bash
# Clone all repos
gh repo clone nexa-net/nexa-core
gh repo clone nexa-net/nexad
gh repo clone nexa-net/nexa-cli

# Build the daemon
cd nexad && cargo build --release

# Build the CLI
cd ../nexa-cli && cargo build --release
```

## Requirements

- Rust 1.85+
- Docker (running)

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
