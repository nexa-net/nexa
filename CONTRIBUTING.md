# Contributing to Helyos

Thank you for your interest in contributing to Helyos! This guide covers the basics.

## Repository Structure

Helyos is a multi-repo project under the [`helyos-labs`](https://github.com/helyos-labs) GitHub organization:

| Repository | Description |
|:--|:--|
| [`helyos`](https://github.com/helyos-labs/helyos) | Documentation, specs, install script |
| [`helyos-core`](https://github.com/helyos-labs/helyos-core) | Core library (domain types, port traits, orchestrator) |
| [`helyosd`](https://github.com/helyos-labs/helyosd) | Daemon (runtime adapters, REST API, gRPC clustering) |
| [`helyos-cli`](https://github.com/helyos-labs/helyos-cli) | CLI tool (deploy, scale, manage) |

## Getting Started

### Prerequisites

- Rust 1.85+ (`rustup update stable`)
- Docker or containerd running on the host
- `protoc` (Protocol Buffers compiler) for building `helyosd`

### Building

```bash
# Clone and build each crate
git clone https://github.com/helyos-labs/helyos-core.git
cd helyos-core && cargo build

git clone https://github.com/helyos-labs/helyosd.git
cd helyosd && cargo build

git clone https://github.com/helyos-labs/helyos-cli.git
cd helyos-cli && cargo build
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

Helyos follows hexagonal architecture (ports and adapters):

- **`ports/`** — Trait definitions (interfaces)
- **`adapters/`** — Concrete implementations
- **`domain/`** — Core business logic, independent of infrastructure

## Reporting Issues

Open issues on the relevant repository. Include:

- Steps to reproduce
- Expected vs actual behavior
- Helyos version (`helyos --version`, `helyosd --version`)
- OS and architecture

## License

By contributing, you agree that your contributions will be licensed under the Apache-2.0 License.
