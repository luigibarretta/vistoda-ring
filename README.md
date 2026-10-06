# Vistoda Ring

Private Rust provider for Ring Intercom controls, events and audio in Vistoda.

The Rust package and executable remain `ring-intercom-bridge` as a compatibility
contract for existing images, health checks and automation. The product and
canonical repository are Vistoda Ring and `vistoda-ring`.

## Current status

The released provider supports password/SMS enrollment, multiple intercom
discovery, status, volume, one-shot unlock, event history, push events and
full-duplex browser audio. Native Ring doorbells and cameras add experimental,
signaling-only live video ([native cameras](docs/NATIVE_CAMERAS.md)).
Recordings contain audio captured during an active Vistoda session; Ring cloud
recording is not required.

Ring supports only IFTTT and Alexa as official third-party integrations. This
provider therefore uses experimental consumer APIs and can stop working after a
vendor change. Keep the Ring app for account recovery. Unlock requires an exact
physical-device binding and is never retried. The Vistoda panel confirms it;
authorized Home Assistant buttons and automations can invoke it directly.

For Home Assistant OS, use the shared
[setup guide](https://github.com/luigibarretta/vistoda-addons/blob/main/GETTING_STARTED.md).
Normal setup discovers intercoms by name and location after login; numeric ID
copying is not required. See [multiple intercoms](docs/MULTI_INTERCOM.md).

## Architecture

```text
Ring cloud -> Rust provider -> session/media boundary
                              +-> WebRTC for HA browser
                              +-> receive-only audio for SceneTrove
                              +-> PCMU relay for native Apple clients
                              +-> bounded native push-event cursor

Vistoda browser -> authenticated HA proxy -> bounded private call archive

Static config -> authenticated capability API -> consumers
```

Home Assistant may switch controls between its official Ring integration and
the native bridge. Unlock is a one-shot command and is never retried by either
consumer or bridge. Native ding and unlock push events are preferred; the
official integration remains a deduplicated fallback until a real doorbell
canary proves the owned event path end to end.

## API

| Endpoint | Purpose | Authentication |
| --- | --- | --- |
| `GET /healthz` | liveness, version and research phase | none |
| `GET /v1/devices` | alias-only inventory and capabilities | bearer |
| `GET /v1/devices/{alias}/capabilities` | verified media capability set | bearer |
| `GET /v1/devices/{alias}/status` | battery, online state, volumes and latest activity | bearer |
| `POST /v1/devices/{alias}/unlock` | one-shot native door unlock | bearer |
| `PATCH /v1/devices/{alias}/settings` | set exactly one bounded volume | bearer |
| `GET /v1/devices/{alias}/events` | cursor/long-poll native ding and unlock events | bearer |
| `GET /v1/devices/{alias}/history` | paginated Ring event history and safe identity | bearer |
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

Native push needs outbound HTTPS plus TCP/5228 to `mtalk.google.com`. Its
registration and acknowledged persistent IDs are atomically stored as a 0600
file. `/healthz` reports only whether the push socket is connected; Prometheus
exports aggregate reconnect/error/event counters without payload or device IDs.
The token is registered with `PATCH clients_api/device` in the official
Android app's record format (payload dictionary 2.4.0, app brand, hardware ID,
notification status, app identity headers) and re-sent after every Ring
session registration, which otherwise drops it.

Every response carries a server-generated `x-request-id`. Failed requests log
only that ID, method, normalized route, status, latency and a bounded error
class; raw URIs, query values, bodies and authorization data are excluded.

Recordings are produced only while a user-owned Vistoda WebRTC session is
active. The browser mixes inbound audio and its microphone only when that
microphone is explicitly enabled, then uploads through Home Assistant's
authenticated backend proxy. Files are atomic, individually capped at 8 MiB,
retained for 30 days and capped to 512 MiB total. The managed app keeps private
storage as the default and lets the user select app-config, media, share or a
live HAOS-managed NFS/Samba mount. Network paths are bounded to one Media or
Share mount and fail closed when the mount is absent. The authenticated
inventory reports the effective directory and exact per-recording path;
standalone deployments can set separate runtime and display paths.

See [`openapi.yaml`](openapi.yaml). Browsers must use an authenticated backend
proxy; they never receive the bridge token.

The native relay accepts exactly 160-byte client PCMU frames and emits bounded
Ring PCMU payloads. It sends silence while the microphone is muted, shares
direct-session exclusivity and cooldown, and expires after 120 seconds.

## Installation and recovery

Start with the shared [English setup guide](https://github.com/luigibarretta/vistoda-addons/blob/main/GETTING_STARTED.md)
or [guida italiana](https://github.com/luigibarretta/vistoda-addons/blob/main/GETTING_STARTED.it.md).
Account reconnection, updates, rollback, restore and uninstall are in the
[operations guide](https://github.com/luigibarretta/vistoda-addons/blob/main/OPERATIONS.md).
The [compatibility matrix](https://github.com/luigibarretta/vistoda-addons/blob/main/COMPATIBILITY.md)
defines the tested release set.
Published images include licenses and notices under `/usr/share/doc/vistoda`.
Only exact version tags passing quality, security and provenance gates are released.

## Development

Read the family [contribution guide](https://github.com/luigibarretta/vistoda-home-assistant/blob/main/CONTRIBUTING.md)
first to understand repository ownership and cross-repository release order.

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
The Home Assistant bootstrap is vendored from
[`lib-vistoda-provider-kit`](https://git.luigibarretta.com/luigibarretta/lib-vistoda-provider-kit)
at the commit in `dependencies/vistoda-provider-kit.sha` and verified byte-for-byte in CI.

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
- [`docs/MULTI_INTERCOM.md`](docs/MULTI_INTERCOM.md) — explicit entrance bindings and safe archive migration;
- [`docs/adr/`](docs/adr/) — durable architectural decisions.

## Author, support and independence

Vistoda Ring is maintained by [Luigi Barretta](https://github.com/luigibarretta).
[Support the project on Ko-fi](https://ko-fi.com/luigibarretta). Vistoda is an
independent project; read the shared [disclaimer](https://github.com/luigibarretta/vistoda-home-assistant/blob/main/DISCLAIMER.md)
and [accessibility statement](https://github.com/luigibarretta/vistoda-home-assistant/blob/main/ACCESSIBILITY.md).

Licensed under Apache-2.0.
