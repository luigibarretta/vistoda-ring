# ADR 0003: Fixture-first protocol research

- Status: accepted
- Date: 2026-08-14

## Context

The official application supports Intercom Audio calls, but no supported API
documents their signalling. Trial-and-error against the live account risks
lockout, throttling, privacy leaks and accidental device actions.

## Decision

Research begins with one operator-controlled trace and immediately reduces it
to synthetic, redacted fixtures. Parsers and state transitions must pass offline
tests before another live request is allowed. There is no brute force of
credentials, identifiers, endpoints or message fields.

## Consequences

- repeatable tests do not need a Ring account;
- raw captures never enter Git;
- every new live probe has a stated hypothesis and hard request/time bound;
- certificate pinning or unavailable observability may block the phase rather
  than justify weakening production verification.
