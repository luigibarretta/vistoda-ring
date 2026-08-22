# ADR 0012: Native Ring status and controls

- Status: Accepted
- Date: 2026-08-22
- Supersedes: the read-only scope in ADR 0007 and access-control separation in ADR 0005

## Context

Vistoda must expose battery, connectivity, the three Ring Intercom volumes and
door unlock without requiring Home Assistant's official Ring integration.
Audio, recordings and controls must share one rotating provider session.

## Decision

The Rust service owns one lazy `RingProvider`. It loads the persisted session
only on first provider operation, so an unenrolled service can still start and
serve enrollment. Audio, recording, status and controls reuse the same client.

`GET /status` returns bounded battery, online state, volumes and last activity.
`PATCH /settings` accepts exactly one volume within the vendor range. `POST
/unlock` issues one command and is never retried above the client's single
authentication-refresh retry. Aliases and bearer authentication are validated
before provider access.

## Consequences

- Vistoda can operate controls independently of the official HA integration.
- HA may retain the official integration as an explicit rollback path.
- Automated live tests may write the current volume back unchanged, but never
  invoke unlock.
- Native push-event parity is a separate gated migration.
