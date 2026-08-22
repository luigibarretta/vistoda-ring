# ADR 0014: Local Vistoda call recording archive

- Status: accepted
- Date: 2026-08-22
- Supersedes: ADR 0011

## Context

The owned Ring Intercom account does not expose Call Recording in the official
application. A cloud post-ding import therefore cannot satisfy the product
contract and produced a misleading eligibility error. Vistoda already owns the
authenticated browser WebRTC session and can capture exactly the media the user
is hearing and, while enabled, transmitting.

## Decision

The browser records only during an active Vistoda communication. Web Audio
mixes the remote track with the local microphone only while that microphone is
enabled. MediaRecorder emits WebM/Opus where supported and MP4 audio as the
bounded fallback. Manual and global automatic recording use the same recorder.

The browser sends media through the authenticated Home Assistant WebSocket
boundary; it never receives the bridge bearer token. Home Assistant base64
decodes at most 8 MiB and forwards one raw media body with bounded start/end
timestamps. The Rust bridge validates MIME type, container signature, size and
call window before atomic commit. Files are private, retained for 30 days and
the archive is capped at 512 MiB.

Consumers list and fetch recordings with bridge authentication. SceneTrove
must acknowledge a successful local commit with
`DELETE /v1/devices/{alias}/recordings/{id}`. The operation is idempotent and
returns `204` when the recording is already absent.

## Consequences

- no Ring recording subscription, setting or spoken provider notice is assumed;
- app-originated Ring calls cannot be recorded by Vistoda unless their media is
  also routed through an active Vistoda browser session;
- recording needs an active page because background Home Assistant automations
  cannot safely obtain a browser microphone;
- an interrupted upload creates no manifest, and archive deletion is explicit.
