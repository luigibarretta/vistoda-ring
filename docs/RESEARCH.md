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

## Current inference

The official app likely uses a cloud-mediated real-time session distinct from
the camera `LiveCall` path. WebRTC is plausible but must not be asserted until a
redacted owned-device trace proves signalling and media transport.

## Unknowns

- signalling endpoint, ticket type and session lifetime;
- whether calls can start on demand or only while a ding is active;
- ICE/TURN requirements and codec set;
- whether receive-only negotiation is accepted;
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
- <https://github.com/tsightler/ring-mqtt/wiki>
