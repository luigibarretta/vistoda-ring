# Operations

## Current phase

The HTTP service advertises verified audio after repeated owned-device canaries.
Calls remain on demand, authenticated, single-device and limited to 120 seconds.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `RING_INTERCOM_BIND_HOST` | `0.0.0.0` | listener address |
| `RING_INTERCOM_BIND_PORT` | `8775` | listener port |
| `RING_INTERCOM_API_TOKEN_FILE` | `/run/secrets/api_token` | bearer token file, at least 32 bytes |
| `RING_INTERCOM_DEVICES_FILE` | `/config/devices.json` | alias-only device kinds |
| `RING_INTERCOM_SESSION_FILE` | `/data/ring-session.json` | dedicated rotating session |

The devices file contains no Ring ID or credential. Only
`ring_intercom_audio` is accepted during the research phase.

## Dedicated Ring session

The research library accepts a separate JSON document with schema version 1,
a stable UUID `hardware_id` and one refresh token. Unknown keys, including a
password, are rejected. On Unix the file must be regular, must not be a symlink
and must have no group or other permissions (normally mode `0600`). Reads are
limited to 16 KiB and secret buffers are zeroed when dropped.

The same bounded client supplies on-demand HTTP audio sessions. Its explicit
`research-discover` command refreshes the dedicated session, registers it,
reads `ring_devices` once and creates a new synthetic fixture containing only
the Intercom Audio count and synthetic identities. It refuses to overwrite an
existing output.

```bash
ring-intercom-bridge research-discover \
  --session-file /private/runtime/ring-session.json \
  --output ./ring-intercom-discovery.synthetic.json
```

Do not run this command until an explicitly enrolled, revocable session exists.
No real session belongs in the repository, and enrollment never scrapes Home
Assistant `.storage`.

## Audio canary

`research-audio-canary` uses the enrolled session and exact discovered
Intercom. It requests one signaling ticket and runs one PCMU `sendrecv` WebRTC
call for 5–30 seconds. The return track is silence, not a microphone. Output is
semantic JSON only; SDP, ICE, tickets and identifiers are never printed.

```bash
RUST_LOG=ring_intercom_bridge=error,webrtc=off,rtc=off \
  ring-intercom-bridge research-audio-canary \
  --session-file /private/runtime/ring-session.json --seconds 5
```

Run it manually, never from a restart policy or healthcheck. Stop after any
vendor `401`, `403`, `429`, account warning or non-zero remote close code. A
successful result requires `session_created`, `answer_received`,
`bidirectional_negotiated`, `peer_connected`, inbound RTP, outbound silence and
`teardown: complete`.

The owned-device release gate completed four consecutive ten-second runs with
inbound RTP and outbound silence on 2026-08-15. Do not schedule further
canaries; consumer contract tests are the next gate.

After deployment, `research-api-canary` tests the complete authenticated HTTP
consumer path from inside the hardened container. It accepts only a loopback
HTTP origin, reads the mounted API token, negotiates one listen-mode peer and
requires inbound PCMU, outbound silence, `DELETE 204` after worker teardown and
local peer teardown.

```bash
docker exec ring-intercom-bridge ring-intercom-bridge research-api-canary \
  --seconds 10
```

## Home Assistant enrollment

The intended operator path is the native Home Assistant Config Flow. HA sends
the password once to `POST /v1/enrollments`, prompts for the SMS code only when
the bridge returns `next_step=otp`, then calls the single-use verification
endpoint. HA persists only bridge address, bridge API token and device alias.

Pending password state expires after 120 seconds. Only one enrollment may be
active, starts have a ten-second cooldown, cancellation is idempotent and a
verification attempt consumes its challenge whether it succeeds or fails. The
bridge never retries rejected credentials, MFA or HTTP 429 responses.

## Safe smoke test

1. Create a 32-byte random API token outside Git.
2. Copy `deploy/devices.example.json` to a non-repository runtime directory.
3. Bind to loopback.
4. Query `/healthz` without authentication.
5. Query `/v1/devices` with the bearer token and require both audio capabilities
   and `phase=verified`.
6. Submit only a fully gathered audio-only PCMU offer through Vistoda or an
   approved backend, then always send the idempotent session `DELETE`.

## Failure behaviour

- missing/short token: startup fails;
- missing/invalid devices file: startup fails;
- unsupported device kind: startup fails;
- invalid bearer: `401` with no detail;
- unknown alias: `404`;
- Ring/network outage during enrollment or session start: stable `502`;
- rejected credentials/code: stable `422`, with no automatic retry;
- expired/consumed challenge: `410`; concurrent flow: `409`; throttling: `429`.

Audio sessions accept one peer per alias and never authorize door actions.
