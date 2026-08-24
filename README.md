# Vistoda Ring

Production Rust connector for bounded Ring Intercom Audio delivery to Home
Assistant and SceneTrove.

The Rust package and executable remain `ring-intercom-bridge` as a compatibility
contract for existing images, health checks and automation. The product and
canonical repository are Vistoda Ring and `vistoda-ring`.

## Current status

Version `0.8.x` exposes bounded direct WebRTC and native PCMU relay APIs after the
owned audio-only Intercom completed repeated bidirectional PCMU canaries. It
adds native battery/status, volume and one-shot unlock contracts without a
public listener. It also archives audio captured by an active Vistoda browser
session; it does not depend on a Ring cloud recording feature.
An authenticated, rate-limited enrollment API can create the dedicated
session through Ring's normal password and SMS-MFA flow; retained pending
password state is zeroizing and credentials are never written to configuration.
Explicit research subcommands still provide refresh-token-only discovery and a
bounded audio canary; neither is invoked by the service. Direct HTTP sessions
accept one fully gathered, audio-only PCMU offer, keep signaling alive for at
most two minutes and close idempotently. A private WebSocket relay lets watchOS
use the same audio through Home Assistant without WebRTC or bridge secrets.

The official Ring application provides two-way audio for Intercom Audio. The
bridge combines that media path with bounded native status and control calls;
protocol research stays behind explicit ADR gates.

## Architecture

```text
Ring cloud -> Rust provider -> session/media boundary
                              +-> WebRTC for HA browser
                              +-> receive-only audio for SceneTrove
                              +-> PCMU relay for native Apple clients

Vistoda browser -> authenticated HA proxy -> bounded private call archive

Static config -> authenticated capability API -> consumers
```

Home Assistant may switch controls between its official Ring integration and
the native bridge. Unlock is a one-shot command and is never retried by either
consumer or bridge. Ding and unlock push events remain delegated during the
native event-stream migration.

## API

| Endpoint | Purpose | Authentication |
| --- | --- | --- |
| `GET /healthz` | liveness, version and research phase | none |
| `GET /v1/devices` | alias-only inventory and capabilities | bearer |
| `GET /v1/devices/{alias}/capabilities` | verified media capability set | bearer |
| `GET /v1/devices/{alias}/status` | battery, online state, volumes and latest activity | bearer |
| `POST /v1/devices/{alias}/unlock` | one-shot native door unlock | bearer |
| `PATCH /v1/devices/{alias}/settings` | set exactly one bounded volume | bearer |
| `POST /v1/enrollments` | start an explicit password/MFA enrollment | bearer |
| `POST /v1/enrollments/{id}` | consume one SMS code and persist the session | bearer |
| `DELETE /v1/enrollments/{id}` | idempotently discard pending secrets | bearer |
| `POST /v1/devices/{alias}/audio/sessions` | negotiate bounded WebRTC audio | bearer |
| `DELETE /v1/devices/{alias}/audio/sessions/{id}` | end audio after local teardown, idempotently | bearer |
| `GET /v1/devices/{alias}/audio/relay` | bounded `vistoda.pcmu.v1` WebSocket audio | bearer |
| `POST /v1/devices/{alias}/recordings` | commit one bounded local WebM/MP4 call | bearer |
| `GET /v1/devices/{alias}/recordings` | list private archive metadata | bearer |
| `GET /v1/devices/{alias}/recordings/{id}` | read one bounded WebM/MP4 | bearer |
| `DELETE /v1/devices/{alias}/recordings/{id}` | acknowledge and remove, idempotently | bearer |
| `GET /metrics` | aggregate session counters and latency histograms | none, private network |

The container healthcheck uses the bounded public `/healthz` endpoint and does
not read or expose the API token or Ring session.

Every response carries a server-generated `x-request-id`. Failed requests log
only that ID, method, normalized route, status, latency and a bounded error
class; raw URIs, query values, bodies and authorization data are excluded.

Recordings are produced only while a user-owned Vistoda WebRTC session is
active. The browser mixes inbound audio and its microphone only when that
microphone is explicitly enabled, then uploads through Home Assistant's
authenticated backend proxy. Files are atomic, private, individually capped at
8 MiB, retained for 30 days and capped to 512 MiB total.

See [`openapi.yaml`](openapi.yaml). Browsers must use an authenticated backend
proxy; they never receive the bridge token.

The native relay accepts exactly 160-byte client PCMU frames and emits bounded
Ring PCMU payloads. It sends silence while the microphone is muted, shares
direct-session exclusivity and cooldown, and expires after 120 seconds.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
cargo audit --deny warnings
docker build -t ring-intercom-bridge:test .
docker run --rm --network none --read-only ring-intercom-bridge:test --version
```

The declared MSRV is Rust 1.96, required by the audited WebRTC dependency
graph and enforced by the production builder image.

Every maintained source or documentation file is limited to 250 lines. Python,
JavaScript and TypeScript sources are rejected by the repository test suite.

## Security

- never store Ring credentials in source, fixtures, logs or command history;
- never brute-force credentials, device IDs, endpoints or protocol fields;
- capture only traffic generated by the owner's account and device;
- bound every future call, stream, queue and retry;
- keep the service private; browsers never receive Ring tokens.
- proxy native relay traffic through an authenticated trusted backend; Apple
  clients receive neither Ring credentials nor the bridge token;
- expose enrollment only through a trusted backend such as the Home Assistant
  Config Flow; never publish the bridge directly.

See [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md) and
[`SECURITY.md`](SECURITY.md).

## Documentation

- [`docs/PLAN.md`](docs/PLAN.md) — staged delivery gates;
- [`docs/RESEARCH.md`](docs/RESEARCH.md) — verified facts and unknowns;
- [`docs/OPERATIONS.md`](docs/OPERATIONS.md) — safe local operation;
- [`docs/adr/`](docs/adr/) — durable architectural decisions.

Licensed under Apache-2.0.
