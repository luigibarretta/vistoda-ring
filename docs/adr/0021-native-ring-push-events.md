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
