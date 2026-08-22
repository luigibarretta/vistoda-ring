# ADR 0015: Native Apple audio relay boundary

- Status: accepted
- Date: 2026-08-22
- Extends ADR 0010

## Context

The browser and SceneTrove paths in ADR 0010 can carry media directly because
their platforms provide WebRTC. watchOS provides the audio, networking,
CallKit and PushKit APIs needed for a native communication client, but the
maintained WebRTC frameworks used by this product do not ship a watchOS target.
Embedding a second, unverified WebRTC stack in the Watch application would
increase binary size and move Ring-specific ICE and SDP behavior onto the least
observable client. Home Assistant Companion can mirror a notification to the
Watch but cannot provide full-duplex Ring audio.

The existing owned-device canary already proves that the Rust service can be
the Ring WebRTC peer and receive/send PCMU. A narrowly scoped relay can reuse
that production path without transcoding.

## Decision

The direct WebRTC API remains the default for browsers and SceneTrove. A second
authenticated WebSocket endpoint, `GET /v1/devices/{alias}/audio/relay`, serves
native Apple clients through a trusted backend proxy.

The bridge remains the only Ring WebRTC peer. The relay protocol is
`vistoda.pcmu.v1`: Ring-to-client binary messages contain a bounded raw PCMU
RTP payload; client-to-Ring binary messages contain exactly one 160-byte,
20-millisecond PCMU frame. Text messages carry only bounded lifecycle state,
ping, stop and stable error codes. No SDP, ICE candidate, Ring identifier,
vendor token or bridge bearer crosses the application boundary.

The relay and direct WebRTC paths share the same per-alias exclusivity gate,
ten-second cooldown and 120-second lifetime. Queues are bounded and use
real-time drop semantics. When microphone data is absent, the bridge sends a
PCMU silence frame instead of blocking Ring media. Disconnect and stop always
close signaling and the peer before the cooldown is committed.

Home Assistant terminates user authentication and proxies WebSocket frames to
the private bridge using the config-entry token. The Apple client authenticates
only to Home Assistant. The Watch starts muted and enables capture only after
an explicit user action; receive and transmit then remain simultaneous.

## Consequences

- watchOS needs no third-party WebRTC runtime or Ring protocol code;
- relay audio is codec-preserving and has bounded memory and lifetime;
- the bridge carries media for native clients, unlike the direct browser path;
- PCMU quality is intentionally limited to Ring's negotiated 8 kHz codec;
- metrics expose aggregate frames and drops, never session or device labels;
- browser and SceneTrove behavior remains compatible with ADR 0010;
- native CallKit/PushKit delivery is a client concern and cannot expose the
  private bridge publicly.
