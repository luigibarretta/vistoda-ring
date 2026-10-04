# Vistoda fork notes

This directory derives from crates.io `fcm-push-listener-ring` 4.0.3 (MIT).
The original repository URL embedded in that crate was unavailable when the
fork was reviewed on 2026-08-25. `UPSTREAM.md` preserves the packaged readme.

Vistoda maintains five bounded changes:

- use the repository's existing reqwest 0.13 and rustls/AWS-LC stack;
- remove the unsafe numeric enum transmute;
- parse named Web Push parameters with length, ASCII and padding checks so a
  malformed Ring push returns an error instead of panicking;
- decrypt RFC 8291 `aes128gcm` payloads when the legacy `crypto-key` and
  `encryption` headers are absent (4.0.3-vistoda.2);
- surface an undecryptable data stanza as `Message::Undecryptable` with its
  persistent ID instead of failing the stream, so one bad message cannot drop
  the connection and every event behind it (4.0.3-vistoda.2).

The vendored boundary exists because Android device type and `com.ringapp`
registration are hard-coded by the specialized crate. Replace it with the
general upstream crate only after those values become configurable and a real
Intercom push canary passes.
