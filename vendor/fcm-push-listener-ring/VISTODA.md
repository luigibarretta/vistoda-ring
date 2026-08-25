# Vistoda fork notes

This directory derives from crates.io `fcm-push-listener-ring` 4.0.3 (MIT).
The original repository URL embedded in that crate was unavailable when the
fork was reviewed on 2026-08-25. `UPSTREAM.md` preserves the packaged readme.

Vistoda maintains three bounded changes:

- use the repository's existing reqwest 0.13 and rustls/AWS-LC stack;
- remove the unsafe numeric enum transmute;
- parse named Web Push parameters with length, ASCII and padding checks so a
  malformed Ring push returns an error instead of panicking.

The vendored boundary exists because Android device type and `com.ringapp`
registration are hard-coded by the specialized crate. Replace it with the
general upstream crate only after those values become configurable and a real
Intercom push canary passes.
