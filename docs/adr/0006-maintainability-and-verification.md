# ADR 0006: Maintainability and verification

- Status: accepted; line limit superseded on 2026-11-07
- Date: 2026-08-14

Update 2026-11-07: the fleet standard (repo-tooling 0.6.0, `fleet-default`
preset) replaces the 250-line rule and `tests/loc_budget.rs`: one 400-line
limit for source files (Markdown and configuration are no longer counted),
zero growth and a ratchet, enforced by the `loc` and `nasa` ci-local steps.
The Python/JavaScript/TypeScript ban stays, as the `source-languages` step.

## Context

Protocol bridges tend to accumulate packet parsing, retries, media conversion,
HTTP handlers and vendor workarounds in oversized modules. Earlier household
projects established explicit LOC and canary requirements.

## Decision

Every maintained Rust, configuration and Markdown file is limited to 250 lines.
CI enforces formatting, strict Clippy lints, all tests, dependency audit,
container build and a network-disabled non-root smoke test. Live canaries are
separate from deployment and have request, byte and duration limits.

## Consequences

- modules must retain narrow ownership;
- large generated files are not committed without an explicit exception ADR;
- deployment success never substitutes for a media canary;
- a canary failure preserves inspectable state and never triggers restart loops.
