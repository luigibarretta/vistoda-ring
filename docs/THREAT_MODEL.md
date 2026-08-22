# Threat model

## Protected assets

- Ring refresh/access tokens, cookies and authenticated device registration;
- household visitor and resident audio;
- Intercom identifiers, topology, SDP and ICE candidates;
- account availability and vendor rate-limit standing;
- physical access control associated with the Intercom.

## Trust boundaries

Ring cloud is the vendor-facing boundary. The Rust provider is the only
component allowed to cross it. Home Assistant and SceneTrove are authenticated
LAN consumers. Home Assistant owns user confirmation and the incoming-ding
automation; Vistoda may execute a bounded native unlock.

## Threats and controls

| Threat | Control |
| --- | --- |
| Unauthorized listening | Constant-time bearer auth, private bind/firewall and no public route |
| Accidental door action | One-shot endpoint, explicit confirmation, no retries after an ambiguous mutation |
| Credential disclosure | Ephemeral zeroizing enrollment state, session file only, no payload logs and synthetic fixtures |
| Account lockout | One active flow, start cooldown, single-use MFA and no retry of rejected requests or 429 |
| Vendor throttling | One call per alias, ten-second post-call cooldown, explicit user start and no rejected-request retry |
| Endless live call | 120-second deadline, disconnect cancellation and server-side teardown |
| Audio memory exhaustion | Peer media is bounded; local uploads are limited to 8 MiB and archive retention to 512 MiB |
| Packet/parser abuse | Length limits, typed state machine and no unsafe Rust |
| SSRF/path traversal | Static alias map and no caller-provided upstream URL |
| Supply-chain compromise | Locked dependencies, CI audit, immutable image digest and non-root runtime |

## Deliberate exclusions

The bridge is not a public telephone service, continuous recorder, surveillance
NVR, identity provider or general Ring account manager. It will not bypass MFA,
certificate validation or vendor authorization controls.
