# ADR 0023: sealed Ring state and the shared provider kit crate

- Status: Accepted
- Date: 2026-11-11
- Refines: [ADR 0005](0005-session-and-access-control-boundaries.md),
  [ADR 0021](0021-native-ring-push-events.md)

## Context

The 2026-11 fleet audit found two gaps compared with the other Vistoda
bridges. `/metrics` was served without the bearer because authentication was
called by each handler and that handler did not (R-M1). The Ring OAuth refresh
token (`ring-session.json`) and the FCM registration (`ring-push.json`) were
plaintext `0600` files, while Blink seals its provider store (R-M2). The same
bearer check and private-file code existed in all three bridges.

## Decision

The bridge depends on the `vistoda-provider-kit` crate of
lib-vistoda-provider-kit, vendored under `vendor/` with the lib-auth-modules
archive it uses, at the kit pin and verified by `check-vendored-dep`.

- One router-wide `require_auth` layer authenticates every route except
  `/healthz`; rejections keep the bridge's `unauthorized` error body. Handlers
  no longer receive or check the `Authorization` header.
- The session and the push registration live in `ring-session.sealed` and
  `ring-push.sealed` next to the configured paths: envelope `{version, nonce,
  ciphertext}`, AES-256-GCM, keys = HKDF sub-keys `vistoda-ring/session/v1`
  and `vistoda-ring/push/v1` of the 32 random bytes in `store.key` (mode
  `0600`, created once and never replaced).
- `serve` seals a plaintext file from an earlier release at startup (research
  commands on first use), reads it back and only then removes it. A
  plaintext file wins over an existing sealed one (it is newer after a
  downgrade). One with unsafe permissions is kept and stops startup, so an
  enrolled session is never lost.
- A push registration that is empty, oversized or no longer opens (lost key,
  damaged envelope) is quarantined like unparsable content and registered
  again. A session that no longer opens, or whose `store.key` is missing,
  fails closed with a configuration error and never reads as "not enrolled".

## Consequences

Holders of the bridge API token (Home Assistant receives it by discovery)
cannot open the Ring state, and a partial copy of `/data` does not reveal it.
`store.key` sits in the same directory, so a full backup still contains
everything needed: backups of `/data` remain as sensitive as the Ring
account. Downgrading to a release without sealing finds no plaintext session
and requires a new enrollment.
