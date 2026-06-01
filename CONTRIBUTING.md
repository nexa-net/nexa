# Contributing to NexaNet

Thank you for your interest in contributing to NexaNet! This guide covers the basics.

## Repository Structure

NexaNet is a multi-repo project under the [`nexa-net`](https://github.com/nexa-net) GitHub organization:

| Repository | Description |
|:--|:--|
| [`nexa`](https://github.com/nexa-net/nexa) | Documentation, specs, install script |
| [`nexa-core`](https://github.com/nexa-net/nexa-core) | Core library (domain types, port traits, orchestrator) |
| [`nexad`](https://github.com/nexa-net/nexad) | Daemon (runtime adapters, REST API, gRPC clustering) |
| [`nexa-cli`](https://github.com/nexa-net/nexa-cli) | CLI tool (deploy, scale, manage) |

## Getting Started

### Prerequisites

- Rust 1.85+ (`rustup update stable`)
- Docker or containerd running on the host
- `protoc` (Protocol Buffers compiler) for building `nexad`

### Building

```bash
# Clone and build each crate
git clone https://github.com/nexa-net/nexa-core.git
cd nexa-core && cargo build

git clone https://github.com/nexa-net/nexad.git
cd nexad && cargo build

git clone https://github.com/nexa-net/nexa-cli.git
cd nexa-cli && cargo build
```

## Before Submitting a PR

Every repository has its own CI pipeline. Before submitting, make sure all checks pass:

```bash
cargo fmt --check          # formatting
cargo clippy -- -D warnings  # lints
cargo test                  # unit + integration tests
```

## Code Style

- Follow existing patterns in the codebase
- Use `parking_lot` instead of `std::sync::Mutex`/`RwLock`
- Use `anyhow` for application errors, `thiserror` for library errors
- Prefer `tracing` over `println!` for logging
- Write tests for new functionality

## Architecture

NexaNet follows hexagonal architecture (ports and adapters):

- **`ports/`** — Trait definitions (interfaces)
- **`adapters/`** — Concrete implementations
- **`domain/`** — Core business logic, independent of infrastructure

## Reporting Issues

Open issues on the relevant repository. Include:

- Steps to reproduce
- Expected vs actual behavior
- NexaNet version (`nexa --version`, `nexad --version`)
- OS and architecture

## License

By contributing, you agree that your contributions will be licensed under the Apache-2.0 License.
