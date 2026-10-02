# Contributing to Night Owl

Night Owl is licensed under **AGPL-3.0-only** (see `LICENSE`). By
contributing code, docs, or manifests you agree your contribution enters
under the same license. There is no contributor license agreement and no
dual licensing: keep it that way — do not introduce
incompatibly-licensed dependencies (run `cargo deny check licenses`;
policy lives in `deny.toml`).

Practical rules this repo enforces:

- Verify in the lab (k3d), never assert without dated evidence. See
  `docs/en/api/lab.md` for setup and reset; record test evidence with the
  corresponding issue or change, not in the user guide.
- Every behavior change carries unit tests (`cargo test`), `cargo clippy`
  clean, and `cargo fmt`.
- Docs are bilingual (EN+ES): behavior changes update both sides.
- Never commit secrets, credentials, or tokens. Lab secrets are applied
  imperatively as ephemeral Secrets and deleted with the test.
- Keep the operator generic: no tenant, vendor, or SaaS specifics in
  code, CRD descriptions, docs, or examples.
