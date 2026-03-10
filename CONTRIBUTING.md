# Contributing

## Development

### Prerequisites
- Rust (latest stable)
- Docker and Docker Compose

### Running Tests
```bash
cargo test
```

### Building
```bash
cargo build --release
```

### Running Locally
```bash
docker-compose up -d --build
```

## Code Style
Run formatting before submitting:
```bash
cargo fmt
```

## Pull Requests
1. Create a feature branch from `dev`
2. Run tests and ensure they pass
3. Submit a PR to `dev` branch
