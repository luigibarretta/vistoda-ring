# ADR 0010: Consumer WebRTC session boundary

- Status: accepted
- Date: 2026-08-15
- Supersedes the consumer assumptions in ADR 0004

## Context

The owned-device canaries proved Ring Intercom Audio uses bidirectional PCMU
over the ordinary Ring signalsocket protocol. Home Assistant needs interactive
audio and an explicitly authorized microphone. SceneTrove needs listen-only
access without learning Ring or bridge credentials. A server-side RTP relay
would add codec, jitter, fanout and buffering state before either consumer can
be tested, while exposing the private bridge directly to browsers would leak
its network boundary and bearer token.

## Decision

The bridge owns only vendor authentication and signaling. An authenticated
backend consumer submits one fully ICE-gathered, audio-only WebRTC offer. The
bridge forwards it to Ring, collects the answer and bounded remote candidates,
activates the call and retains the vendor signaling socket for at most 120
seconds. Media flows directly between the consumer peer and Ring.

Offers are limited to 64 KiB, exactly one `sendrecv` audio section, PCMU
payload 0, no video and no data channel. At most 64 remote candidates of 4 KiB
are returned. One session per alias is allowed. Start has a 25-second consumer
deadline; every session closes on explicit cancellation, remote close,
signaling failure or the hard lifetime.

`mode=talk` means the trusted consumer obtained microphone permission through
an explicit user action. `mode=listen` still negotiates `sendrecv`, but the
consumer supplies a locally generated silent track. This matches the proven
Ring transport while preserving a receive-only application policy.

`DELETE /v1/devices/{alias}/audio/sessions/{id}` is idempotent. The bridge
token remains in Home Assistant or the SceneTrove backend. Browser code talks
only to its authenticated application backend.

## Home Assistant boundary

Vistoda registers an authenticated Home Assistant WebSocket command set and a
local custom panel. The panel can request listen or talk, but only the talk
button calls `getUserMedia`. It constrains the browser to PCMU, sends the offer
through HA, applies the redacted answer and candidates, and always attempts the
idempotent delete during stop or failure. No bridge URL, bearer token, Ring
session or vendor identity enters JavaScript.

## SceneTrove boundary

SceneTrove can use the same OpenAPI operation with `mode=listen`. Its browser
must generate silence and its backend must proxy start/delete using a
deploy-owned secret file, matching its existing capture-bridge trust model.
Provider-specific Ring signaling does not enter SceneTrove.

## Consequences

- interactive audio does not require transcoding or unbounded server buffers;
- direct ICE reachability is a release gate for every supported browser path;
- one device cannot currently serve multiple simultaneous peers;
- a future relay or fanout is a separate ADR and cannot weaken the limits here;
- capabilities may be advertised only with live API and teardown evidence.
