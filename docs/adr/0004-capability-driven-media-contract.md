# ADR 0004: Capability-driven media contract

- Status: accepted
- Date: 2026-08-14

## Context

Home Assistant needs an interactive browser session while SceneTrove needs a
receive-only ingest stream. Ring Intercom Audio has no video, unlike EZVIZ and
Blink. Pretending all providers are cameras creates false and brittle APIs.

## Decision

The root resource is a device with explicit media capabilities:
receive audio, transmit audio, receive video and recordings. Unverified
capabilities are false. A future interactive endpoint will use WebRTC offer/
answer semantics; a separate receive-only Opus or AAC stream may serve
SceneTrove. No empty video track will be invented for compatibility.

## Consequences

- consumers branch on declared capabilities;
- Home Assistant requires a purpose-built card for microphone interaction;
- SceneTrove must support audio-only device timelines;
- adding Ring Intercom Video later does not change Audio semantics.
