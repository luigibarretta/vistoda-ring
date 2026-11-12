# Changelog

Release notes for the Ring provider. User-facing app notes live in the
Vistoda Apps catalog (`vistoda_ring/CHANGELOG.md`).

## Unreleased

- `/metrics` requires the bearer like every other route except `/healthz`:
  authentication moved from the handlers to one router-wide middleware from
  the shared lib-vistoda-provider-kit crate (audit R-M1). Unknown routes now
  also answer 401 without a valid bearer, and authentication runs before
  request validation.
- The Ring session and the push registration are sealed at rest
  (`ring-session.sealed`, `ring-push.sealed`, AES-256-GCM under HKDF sub-keys
  of a dedicated `store.key`). `serve` seals existing plaintext files at
  startup and removes them only after the sealed copy reads back; a
  plaintext file wins over an older sealed copy (audit R-M2). A missing
  `store.key` fails closed instead of reading as "not enrolled".
  Downgrading after this release requires a new enrollment.
- Private-file and authentication code now comes from the vendored
  lib-vistoda-provider-kit 0.2.0 crate (with lib-auth-modules v0.2.0).

## 0.18.2

- WebRTC stack upgrade; requires a live intercom call test before deploy.
  `webrtc`/`rtc` 0.20.2 -> 0.21.1: DTLS, SRTP and STUN cryptography now run
  through the `ring` provider, so crypto-bigint 0.5.5 and p256 0.13 leave the
  dependency graph and the RUSTSEC-2026-0387 audit exception is removed.
- No behaviour change intended: PCMU 8 kHz payload 0 only, `sendrecv`,
  `setup:actpass` offers, the same STUN servers, bind address and timeouts,
  and the same 160-byte relay framing.
- Offline loopback tests now prove offer/answer, DTLS in both roles, trickled
  ICE candidates and PCMU audio in both directions on the new stack.

## 0.18.1

- Version bump for the custom AppArmor profile; tokio 1.54.1
  (RUSTSEC-2026-0386).
