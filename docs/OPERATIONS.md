# Operations

This is the advanced provider runbook. Home Assistant OS users should begin
with the shared [installation guide](https://github.com/luigibarretta/vistoda-addons/blob/main/GETTING_STARTED.md).

## Current phase

The HTTP service advertises verified audio after repeated owned-device canaries.
Calls remain on demand, authenticated, device-scoped and limited to 120 seconds.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `RING_INTERCOM_BIND_HOST` | `0.0.0.0` | listener address |
| `RING_INTERCOM_BIND_PORT` | `8775` | listener port |
| `RING_INTERCOM_API_TOKEN_FILE` | `/run/secrets/api_token` | bearer token file, at least 32 bytes |
| `RING_INTERCOM_DEVICES_FILE` | `/config/devices.json` | bootstrap aliases or explicit physical bindings |
| `RING_INTERCOM_SESSION_FILE` | `/data/ring-session.json` | dedicated rotating session |
| `RING_INTERCOM_RECORDING_DIR` | `/data/recordings` | private bounded call archive |
| `RING_INTERCOM_RECORDING_DISPLAY_DIR` | `/data/recordings` | user-facing absolute archive path |
| `RING_INTERCOM_RECORDING_STORAGE_KIND` | `private` | `private`, `addon_config`, `media`, `share` or `custom` |

The devices file contains no credential. `ring_intercom_audio` is the supported
kind; advanced mappings can include a positive `device_id`. Normal app setup
uses account discovery after login, not manual ID copying. See
[multiple intercoms](MULTI_INTERCOM.md) for automatic aliases and archive handling.

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
2. Copy `deploy/devices.example.json` to a non-repository runtime directory;
   replace every example ID with a verified Ring device ID and keep only the
   intended entrances. See [multiple intercoms](MULTI_INTERCOM.md) for the
   Home Assistant options, single-device compatibility and archive migration.
3. Bind to loopback.
4. Query `/healthz` without authentication.
5. Query `/v1/devices` with the bearer token and require both audio capabilities
   and `phase=verified`.
6. Submit only a fully gathered audio-only PCMU offer through Vistoda or an
   approved backend, then always send the idempotent session `DELETE`.

## Native Apple relay

The relay WebSocket is private and bearer-authenticated. It is intended only
for Home Assistant's authenticated HTTP proxy; do not publish it through the
external reverse proxy or place its bearer in an iPhone or Watch application.
The server announces `vistoda.pcmu.v1`, sends bounded raw PCMU payloads and
accepts only 160-byte client PCMU frames. A text `{"type":"stop"}` requests a
clean teardown; a socket close has the same bounded effect.

Relay and direct WebRTC calls share one per-device slot and the ten-second
post-call cooldown. While muted, the client sends no audio and the bridge
supplies PCMU silence. Inspect aggregate relay frame/drop counters under
`/metrics`; no device or session identifier is emitted.

## Intercom unlock settings

The official Ring app keeps two Intercom settings under "Unlock Settings" that
decide whether a remote unlock actually opens the door. Once that app is
uninstalled they are invisible, so `GET /v1/devices/{device}/status` reports
them read-only as the optional `unlock_settings` object. The values come from
`settings.intercom_settings` of the Intercom in the device discovery response
the status call already fetches; no extra Ring request is made. The object is
omitted when Ring reports no such settings, and each field is omitted when its
value is missing, malformed or out of range.

- `mode` / `ring_to_open_enabled` mirror the Ring app's "Unlock Type"
  (`intercom_settings.ring_to_open`):
  - `direct` ("Direct Unlock"): the building system opens at any time, so
    Vistoda's open-door command releases the door directly.
  - `ring_to_open` ("Ring-to-Open"): the building system only opens during an
    active call. Ring may accept Vistoda's unlock command while the door stays
    shut unless someone is pressing the call button for the unit at the
    entrance panel. If open-door "succeeds" but nothing happens, check this
    mode first; switching it requires the Ring app.
- `duration_seconds` is the "Unlock Duration" of analog building systems
  (`analog.unlock_duration` inside the `intercom_settings.config` document,
  stored by Ring in milliseconds and reported here in whole seconds; 0 means
  under one second). The Ring app offers 1 to 10 seconds. A very short value
  can release the lock before the door is pushed; the building may also
  override it. Digital systems do not expose it.

The integer `intercom_settings.unlock_mode` is present in Ring's model but is
not read by the Ring app, so its meaning is unknown and it is not reported.

## Intercom unlock events

Ring pushes dings to this client but not unlocks made from the official Ring
app, which it records only in the device event history
(`evm/v2/history/devices` with `capabilities=ringtercom`, items
`Door.Unlock`). The bridge polls that history every 20 seconds (backing off to
five minutes on errors) and publishes new unlocks through the same event
cursor as push, so Home Assistant notifies them up to about 20 seconds late.
Unlocks older than the bridge start are never replayed (after a failing start,
at most 15 minutes late), an Intercom enrolled later starts from its first
poll, and a push and a history report of the same unlock within 15 seconds
are published once. A failing Intercom never blocks the others; each failure
increments `vistoda_ring_unlock_history_errors_total`.
History unlocks carry Ring's `origin` (`user`, `device`, `code`, `delivery`)
and, when known, the display name of who unlocked as `actor`; the name is
returned only on the authenticated event cursor and never logged.

## Push-silence watchdog

The same history poll also reads Intercom dings, which Ring records there as
well. The exact `event_type` of an Intercom ding in this feed is not yet
confirmed, so the bridge accepts `ding` alone or as the last segment
(`Intercom.Ding`, `intercom_ding`, case-insensitive) and items with
`kind=ding`. `GET /v1/devices/{device}/activity-probe` reports the raw
`event_type=…` labels plus `classified=ding` and `classified=unlock` counts
for the history feed, so a live probe after a real ring confirms the match.

A history ding newer than the watcher start is judged 120 seconds after it
occurred while push has proven itself (a push ding was seen and none was
missed after it); otherwise, including right after a start, it is judged 15
seconds after it was first read, because waiting longer for a silent push only
delays the report and a late push still matches it first.
When no push ding for the same Intercom occurred within 90 seconds of it, push
missed it and `vistoda_ring_push_missed_dings_total` increments (one redacted
warning, no IDs or times). History dings are never published as live call
events, because a late "someone is ringing" would mislead; the miss time is
reported instead, so Home Assistant can say when someone rang. The device event cursor reports `push_degraded=true`
while a miss from the last 24 hours has no newer push ding, and
`last_missed_ding_at` (Unix seconds, omitted when none) for the newest miss
since the bridge started. Dings older than one hour when first read are
ignored.

## Revoked Ring session

When Ring rejects the stored refresh token (OAuth HTTP 401, or 400 with
`invalid_grant`), device routes that contact Ring return `403` with
`{"error":"reauth_required"}` (error class `provider_auth`); `401` remains
reserved for the bridge bearer token. Transient OAuth failures (5xx, other
4xx, transport) keep the previous `500 internal` behaviour. The bridge does
not present a rejected token again for 15 minutes, the push listener and the
history poll back off to five minutes, and the state is logged once and
exposed as the `vistoda_ring_reauth_required` gauge. Re-enrolling the account
replaces the session and clears it.

## Local call recording archive

Vistoda records only an active browser communication. Its recorder mixes the
remote WebRTC audio with the microphone track only while the user has enabled
that track. The Home Assistant WebSocket proxy accepts at most 8 MiB of base64,
decodes it and forwards the raw WebM or MP4 body to the authenticated bridge.
Ring Call Recording, a Ring subscription and a provider media import are not
part of this contract.

Keep `/data` on private persistent storage owned by UID/GID 10001. Recording
files are mode `0600`; the recording directory is `0700`. The bridge removes
items older than 30 days and then the oldest items whenever the archive exceeds
512 MiB. SceneTrove must send idempotent `DELETE` after its local commit; a
second delete intentionally returns `204`.

## Failure behaviour

- missing/short token: startup fails;
- missing/invalid devices file: startup fails;
- unsupported device kind: startup fails;
- invalid bearer: `401` with no detail;
- unknown alias: `404`;
- Ring/network outage during enrollment or session start: stable `502`;
- rejected credentials/code: stable `422`, with no automatic retry;
- revoked Ring refresh token: `403` `reauth_required` until re-enrollment;
- expired/consumed challenge: `410`; concurrent flow: `409`; throttling: `429`.
- invalid container, size or timestamps: stable `400` and no file;
- browser upload interruption: no manifest is committed.

Every HTTP 4xx/5xx response includes a server-generated `x-request-id`. The
structured failure log contains only that ID, method, normalized route template,
status, latency and a bounded error class. Raw URIs, query strings, bodies,
authorization headers, aliases and client-supplied request IDs are never logged.

Audio sessions accept one peer per alias and never authorize door actions.
