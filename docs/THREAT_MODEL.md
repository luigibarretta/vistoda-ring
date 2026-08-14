# Threat model

## Protected assets

- Ring refresh/access tokens, cookies and authenticated device registration;
- household visitor and resident audio;
- Intercom identifiers, topology, SDP and ICE candidates;
- account availability and vendor rate-limit standing;
- physical access control associated with the Intercom.

## Trust boundaries

Ring cloud is the future vendor-facing boundary. The Rust provider is the only
component allowed to cross it. Home Assistant and SceneTrove are authenticated
LAN consumers. The supported Home Assistant Ring integration remains the sole
owner of door unlock and ding automations.

## Threats and controls

| Threat | Control |
| --- | --- |
| Unauthorized listening | Constant-time bearer auth, private bind/firewall and no public route |
| Accidental door action | No unlock endpoint, command or Ring RPC dependency |
| Credential disclosure | Files only, no debug payloads, structured redaction and synthetic fixtures |
| Account lockout | No password login loop, no brute force, dedicated revocable session and bounded retries |
| Vendor throttling | Single shared call, hard cooldown, explicit user start and circuit breaker |
| Endless live call | Hard session deadline, idle cancellation and server-side teardown proof |
| Audio memory exhaustion | Bounded jitter/consumer queues and slow-client eviction |
| Packet/parser abuse | Length limits, typed state machine and no unsafe Rust |
| SSRF/path traversal | Static alias map and no caller-provided upstream URL |
| Supply-chain compromise | Locked dependencies, CI audit, immutable image digest and non-root runtime |

## Deliberate exclusions

The bridge is not a lock controller, public telephone service, continuous
recorder, surveillance NVR, identity provider or Ring account manager. It will
not bypass MFA, certificate validation or vendor authorization controls.
