# Protocol research

## Verified facts

- The owned Home Assistant instance exposes the Ring Intercom as unlock, ding,
  unlock-event, battery and volume entities; it exposes no camera/media entity.
- Ring documents two-way talk for Ring Intercom Audio through the official app.
- Home Assistant supports Intercom events, unlock and volume, but not Intercom
  media or two-way audio.
- `ring-client-api` models Intercom separately from `RingCamera`; its Intercom
  class has no `startLiveCall` or WebRTC primitive.
- ring-mqtt explicitly reports no audio/video streaming for Intercom devices.
- Ring has no supported public consumer API for this workflow.
- At upstream commit `638d5285aea5f34d44d9bacbb41917f736764d49e`,
  `ring-client-api` refreshes through the fixed Ring OAuth endpoint with the
  Android client identity, a stable hardware UUID and a rotating refresh token.
- The same revision registers an Android session at `clients_api/session`
  before requesting `clients_api/ring_devices`; its API metadata version is 11.
- `python-ring-doorbell` and `ring-client-api` obtain one ticket from the Ring
  application API, open the version 4 signalsocket WebSocket, send a WebRTC
  `live_view` offer and activate the returned session. Their audio offer
  includes Opus and PCMU with a `sendrecv` return track.
- `ring-intercom-video` demonstrates that the streaming omission on
  `RingOther` is a client class boundary for Intercom Video, not a different
  signaling service.
- An initial bounded owned-device canary and four consecutive ten-second
  repetitions on 2026-08-15 proved the same path on
  `intercom_handset_audio`: bidirectional SDP, connected DTLS/SRTP, inbound
  PCMU RTP, outbound silence and complete teardown without a remote close.
- The maintained canary uses `webrtc` 0.20.2 and passes the strict RustSec
  audit; its semantic output contains no Ring identity, SDP, ICE or audio.

The Rust implementation contains an opt-in discovery command and a separate
5–30 second audio canary. Neither is instantiated by the running HTTP service.

## Current conclusion

Ring Intercom Audio uses Ring's ordinary cloud signalsocket WebRTC path. The
public clients omit it because their Intercom wrapper does not inherit the
camera streaming methods. Consumer lifecycle and relay contracts remain to be
implemented; the vendor media protocol is no longer the unknown.

## Unknowns

- maximum reliable session lifetime and concurrency;
- whether receive-only negotiation is accepted by all firmware revisions;
- Opus behaviour on the owned audio-only model;
- call concurrency and throttling behaviour;
- whether the official app uses certificate pinning;
- whether audio recording is available or subscription-bound.

## Evidence rules

1. Capture only operator-generated traffic from the owned account and device.
2. Store no raw credential, cookie, token, SDP, ICE address or household audio
   in Git.
3. Convert necessary messages into minimal synthetic fixtures.
4. Prove every fixture contains no known secret and no public/private address.
5. Record source version, date and a semantic description, not opaque payloads.
6. Stop on authentication warnings, throttling or unexpected device behaviour.

## Primary references

- <https://www.home-assistant.io/integrations/ring/>
- <https://ring.com/gb/en/support/articles/xwk4u/Ring-Intercom-Support>
- <https://github.com/dgreif/ring/blob/main/packages/ring-client-api/ring-intercom.ts>
- <https://github.com/dgreif/ring/blob/638d5285aea5f34d44d9bacbb41917f736764d49e/packages/ring-client-api/rest-client.ts>
- <https://github.com/tsightler/ring-mqtt/wiki>
- <https://github.com/python-ring-doorbell/python-ring-doorbell/blob/main/ring_doorbell/webrtcstream.py>
- <https://github.com/cmos486/ring-intercom-video>
