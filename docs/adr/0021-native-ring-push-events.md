# ADR 0021: native Ring push events

- Status: Accepted
- Date: 2026-08-25

## Context

Vistoda Ring already owns enrollment, status, controls and full-duplex audio,
but incoming Intercom ding and unlock events were mirrored from Home
Assistant's official Ring integration. That prevented standalone operation and
made Companion call delivery depend on a second cloud session.

Ring's Android client registers a Firebase Cloud Messaging token through
`PATCH /clients_api/device`, subscribes the Intercom through its bounded
`/subscribe` endpoint and receives encrypted messages on the Android MCS
connection. The Firebase application identifiers used for that registration
are public client configuration, not account credentials.

## Decision

The Rust provider owns one persistent FCM registration and MCS listener:

- registration material and acknowledged persistent IDs live in one atomic
  0600 file under the app data directory;
- outbound access is limited to HTTPS provider endpoints and
  `mtalk.google.com:5228`;
- only the exact Intercom ding and unlock categories are accepted, and the
  provider device ID must match the enrolled Intercom;
- raw payloads, registration tokens and device IDs are never logged or exposed;
- events enter a 128-item in-memory queue with a monotonic process-local cursor
  and a random, non-secret process generation;
- consumers use a bearer-authenticated long poll capped at 30 seconds;
- omitting the cursor initializes a consumer at the live tail, so reloading Home
  Assistant cannot replay a queued doorbell event;
- health and Prometheus expose only connection state and aggregate counters.

Home Assistant prefers this native cursor and suppresses a matching official
event for ten seconds. The official listener remains a fallback until a real
doorbell canary proves native delivery. It can then be removed in a superseding
release without changing entity IDs or automations.

## Consequences

Vistoda Ring can receive calls without relying on the official integration,
while existing households retain a safe migration path. A blocked TCP/5228
connection is visible through health, metrics and HA availability rather than
silently losing events. A process restart changes the generation and resets the
public cursor while preserving FCM acknowledgements. Consumers discard the old
cursor and resume at the new live tail, so old provider messages are not
replayed and a stale cursor cannot stall delivery.

## Amendment (0.18.0): make-before-break connection lifecycle

Until 0.17 every ended MCS connection deleted the stored registration and
registered from scratch. On 2026-10-20 Google answered the fresh check-in with
`PHONE_REGISTRATION_ERROR` five times in a row, so a routine disconnect became
a three-minute push outage. The lifecycle is now:

- a plain disconnect reconnects with the stored registration;
- the registration rotates only after three consecutive suspect endings: the
  server closing a connection that was up for less than 60 seconds, or GCM
  check-in refusing the stored credentials;
- a rotation registers new credentials, hands the token to Ring and subscribes
  the Intercoms, and only then atomically replaces the file; the old file is
  never deleted first, and any failed step keeps and uses the old
  registration;
- the GCM token request is retried up to five times, 1-2 seconds apart, on the
  same check-in instead of creating a new Android device per retry;
- the backoff (5 seconds doubling to 5 minutes, plus up to 25% jitter) resets
  only after a connection stayed up for 60 seconds;
- MCS connect and TLS handshake are bounded (10 and 15 seconds), the client
  sends a heartbeat ping every 5 minutes and treats 10 minutes without inbound
  traffic as a dead socket;
- an unparsable registration file is moved aside as `<file>.corrupt` and
  replaced; permission and I/O problems stay fail-closed. `/healthz` reports
  `push_state_reset=true` until the next start;
- Ring refuses the app-format device record (HTTP 400) and accepts the legacy
  2.0.0 record. The refusal is cached per client and re-probed daily; 401 and
  429 never trigger the fallback.

Prometheus adds `vistoda_ring_push_errors_total{stage=…}` (device-change
reloads are not errors), `vistoda_ring_push_rotations_total`,
`vistoda_ring_push_state_resets_total`, `vistoda_ring_push_legacy_record` and
`vistoda_ring_push_last_heartbeat_timestamp_seconds`.
