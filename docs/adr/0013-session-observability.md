# ADR 0013: Ring communication observability

- Status: Accepted
- Date: 2026-08-22

## Context

Intermittent WebRTC startup and teardown failures need correlation without
leaking account, device or session identifiers into metrics.

## Decision

The bridge emits structured start and end logs with bounded termination reasons.
It exposes aggregate Prometheus counters for starts and ends, an active-session
gauge, and histograms for browser ICE gathering and communication duration.
Metrics carry only bounded mode and reason labels; device aliases and UUIDs are
excluded. `/metrics` is public only inside the private service network.

The browser reports its measured ICE gathering duration. A bounded timeout may
continue only when the local SDP already contains a usable candidate; zero
candidates remains a fail-closed error.

## Consequences

- ICE and lifecycle regressions are observable in logs and Prometheus.
- Metrics cardinality is fixed and contains no customer identifiers.
- HA adds its own anonymized event and Logbook correlation for user-visible
  audit, while the bridge remains consumer-neutral.
