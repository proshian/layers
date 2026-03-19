# Layers

## Realtime Collaboration

Run these in two separate terminals to share a project in real time.

**Terminal 1 — server:**
```
cargo run --bin surreal_server
```

**Terminal 2 — first client:**
```
cargo run -- --db-url ws://localhost:8000 --project my-project
```

**Terminal 3 — second client:**
```
cargo run -- --db-url ws://localhost:8000 --project my-project
```

Both clients connect to the same `--project` ID and will sync in real time via the server. Each client starts with an empty state and the server's full op log is replayed to reconstruct the project.
