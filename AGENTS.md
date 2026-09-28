<!-- BEGIN KATA (managed by `kata init --with-agents`) -->
## kata issue tracker

This project uses [kata](https://github.com/kenn-io/kata) as its shared issue
ledger. Run `kata quickstart` at the start of each session for the full agent
contract. The short version:

- Search before creating: `kata search "<keywords>" --agent`.
- Prefer updating existing issues over duplicates (`kata comment`, `kata label add`, `kata edit`).
- Default to `--agent` for ordinary reads and mutations; use `--json` only when a script needs structured data.
- Close only verified work: `kata close <ref> --done --message "<scope + verification>" --commit <sha>`.
- If work is incomplete, label `needs-review` and comment what remains rather than closing.
- Never `kata delete` or `kata purge` without explicit user authorization.

## kata work.* conventions (agent orchestration)

When working a kata-tracked issue, keep its `work.*` metadata truthful
(see docs/operations/agent-orchestration.md for the full recipe):

- On claim/start: `kata meta set <ref> work.attention ok`; if the work has a
  dedicated branch, stamp it once with `kata meta set <ref> work.branch <branch>`.
- Signal live state: `kata meta set <ref> work.attention stuck|needs-human|ok`
  plus a one-line `work.attention_msg` saying why. Raise `stuck` when you cannot
  proceed, `needs-human` when you want review; clear back to `ok` when unblocked.
- Never stop with the signal stale: close the issue, or leave the attention
  pair reflecting the hand-off.
- Coordinators read `work.*` on issues they delegated; only the working agent
  writes them. `work.*` on closed issues is meaningless.
<!-- END KATA -->

## commits

Use Conventional Commits (`type: subject`, e.g. `feat:`, `fix:`, `docs:`).

## engineering conventions

Process source of truth: CONTRIBUTING.md. Code rules followed here:

- Observe, never gate; fail closed with the exact blocker; never invent data.
- Destructive decisions need fresh positive evidence; document staleness bounds where standing values are used.
- Separate pure decisions from IO so they unit-test without a cluster.
- One concern per file with a `//!` why-header; churn-free status writes (skip unchanged, keep transitions, carry evidence).
- Never adopt foreign objects or auto-delete data; deletions belong to humans.
- Lab evidence dated before claiming; EN+ES docs together.

## code layout

Enforced in `src/lib.rs`: `#![deny(clippy::too_many_arguments)]` — bundle
context into a struct (`Build`-style assembler) instead of adding parameters;
never silence it with `#[allow]`. The only `allow` in the tree is
`#[allow(dead_code)]` on CRD schema placeholders.

- One concern per file behind a `//!` why-header; assembler `mod` files only wire, never implement.
- `.expect()` only for pre-established invariants; fallible IO becomes a `Blocked` observation or `Error`. No `unwrap`/`panic!`/`todo!` outside `#[cfg(test)]`.
- Keep files small enough to hold in your head (today max ~900 lines); split by concern before growing.
