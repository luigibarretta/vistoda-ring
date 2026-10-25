# Changelog

Release notes for the Ring provider. User-facing app notes live in the
Vistoda Apps catalog (`vistoda_ring/CHANGELOG.md`).

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
