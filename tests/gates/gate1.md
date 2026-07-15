# Gate 1 (macOS install success)

- Run `cargo test` and `cargo run -- validate`
- Sample set size: 10 logical tool IDs
- Pass threshold: `success_rate >= 0.90`
- Release evidence must have `evidence_kind: real` and an input SHA-256.
- Contract fixtures under `artifacts/contracts` never satisfy this gate.
