# Gate 2 (Linux resolver baseline)

- Verify resolver ranking rules: exact > prefix > contains
- Verify retry accounting in gate summary
- Pass threshold: `success_rate >= 0.60`
- Release evidence must have `evidence_kind: real` and an input SHA-256.
- Contract fixtures under `artifacts/contracts` never satisfy this gate.
