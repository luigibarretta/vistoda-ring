# ADR 0009: Ring signalsocket audio protocol

- Status: accepted
- Date: 2026-08-15

## Context

Ring Intercom Audio supports two-way talk in the official application, but the
public Ring clients model it as `RingOther` and do not attach their camera
streaming methods. A newer community integration proved that Ring Intercom
Video accepts the ordinary Ring signalsocket WebRTC protocol after the same
methods are attached to `RingOther`. The audio-only device still required an
owned-device proof; source similarity alone was insufficient.

The inspected primary revisions were:

- `dgreif/ring` `638d5285aea5f34d44d9bacbb41917f736764d49e`;
- `python-ring-doorbell` `486193a80e7c924a0ab14b04d47305e1b36e419e`;
- `cmos486/ring-intercom-video` `9ed462c28fdf56ca2b945ec800ee1c2d0f507fa7`.

## Decision

The Rust bridge implements an explicit, operator-run audio canary using the
dedicated refresh-token session. It performs one inventory read, requests one
signalsocket ticket from the compiled Ring application endpoint and opens the
compiled TLS WebSocket origin with redirects unavailable.

The canary offers audio only. PCMU at 8 kHz mono is deliberately the sole codec
because the inspected Ring client declares it and it permits deterministic
silence generation without a microphone or codec library. The transceiver is
`sendrecv`: incoming SRTP packets are counted and outgoing payloads contain
only G.711 silence. No household audio, SDP, ICE candidate, ticket, device ID
or session ID is persisted or printed.

Every run is limited to 5–30 seconds. It sends a session close message, closes
WebSocket and peer connection, and emits only structured semantic evidence.
The command is not called by the HTTP service and cannot unlock the door.

## Live evidence

The first five-second canary on the owned `intercom_handset_audio` completed
all signaling stages, negotiated bidirectional audio, established DTLS/SRTP,
received 37 RTP packets (17,760 payload bytes), wrote 227 silence frames and
completed teardown. Ring returned no authentication, authorization or rate
limit error.

The implementation was then migrated from `webrtc` 0.14 to the stable 0.20.2
Sans-I/O stack so `cargo audit --deny warnings` no longer inherits the retired
`bincode` 1.x dependency. Four consecutive ten-second canaries on that stack
received 130, 132, 139 and 76 PCMU packets, sent 485 silence frames per run,
reported no remote close code and completed teardown. The preceding five-second
warm-up negotiated and sent successfully but received no RTP, so it was not
counted toward the receive gate.

## Consequences

- the protocol research gap is closed for audio signaling and transport;
- the repeated receive and full-duplex-silence canary gates are complete;
- Home Assistant and SceneTrove can share WebRTC offer/answer semantics;
- real microphone audio requires an explicit interactive consumer action;
- the production service remains fail-closed until session lifecycle, fanout,
  rate limit and consumer API tests pass.
