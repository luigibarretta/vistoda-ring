# ADR-0008: Native Home Assistant enrollment boundary

## Context

The bridge needs a dedicated Ring session, but asking an operator to copy a
refresh token from a terminal is unsuitable for a Home Assistant product.
Putting Ring credentials or the rotating session in a Home Assistant config
entry would create a second session owner and expand the secret boundary.

## Decision

The Rust bridge exposes bearer-authenticated start, verify and cancel enrollment
operations. The Home Assistant integration is a thin Config Flow client:

1. it sends email and password once from the HA backend to the private bridge;
2. the bridge performs exactly one Ring password-grant request;
3. when Ring requires SMS MFA, the bridge retains password state in zeroizing
   memory for at most 120 seconds;
4. one verification request consumes the challenge before any vendor I/O;
5. success atomically persists only stable hardware ID and rotating refresh
   token in a mode-0600 bridge-owned file;
6. cancel is idempotent and immediately drops pending secrets.

Only one enrollment may exist at a time. Starts have a local cooldown. Invalid
credentials, invalid MFA and HTTP 429 are never retried. The public contract
uses stable redacted error codes and never returns vendor payloads, masked phone
numbers, tokens or device IDs.

Home Assistant stores only the private bridge endpoint, its independent API
token and a configured alias. The existing official Ring integration remains
the owner of events and unlock; this bridge exposes no access-control action.

## Consequences

- enrollment has the native HA user experience without making HA the vendor
  session owner;
- process loss or expiry requires a fresh password step, avoiding durable
  pending credentials;
- one mistyped MFA code requires a fresh challenge by design;
- CLI tooling remains a break-glass research path, not the primary workflow;
- the bridge must remain private and reachable only by approved consumers.
