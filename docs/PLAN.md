# Delivery plan

Each phase is a hard gate. Later phases may not claim completion from source
code alone; they require bounded evidence from the owned device.

## Phase 0 — repository and safety boundary

- [x] Rust-only repository with Apache-2.0 licensing.
- [x] CI, dependency audit, container smoke test and 250-line file budget.
- [x] Threat model and ADRs.
- [x] Authenticated, fail-closed capability API.
- [ ] Gitea repository and private GitHub push mirror.

Exit criterion: clean CI and no Ring session or credential dependency.

## Phase 1 — protocol evidence

- [ ] Inventory the exact Intercom Audio device kind through a redacted fixture.
- [ ] Identify the official app call-signalling sequence.
- [ ] Define a redaction format for HTTP/WebSocket message fixtures.
- [ ] Replay parsing and state transitions entirely offline.

Exit criterion: deterministic fixture tests explain authentication, signalling,
call start, keepalive and call termination without containing secrets.

## Phase 2 — receive-only canary

- [ ] Enrol one dedicated, revocable Ring session.
- [ ] Start one operator-triggered call with a 30-second hard limit.
- [ ] Receive authenticated RTP audio and verify codec/timestamps.
- [ ] Stop and prove all tasks, sockets and keepalives terminate.

Exit criterion: three consecutive bounded canaries, no account warnings, no
door action and no residual live session.

## Phase 3 — full-duplex audio

- [ ] Negotiate microphone return audio with explicit user action.
- [ ] Add echo-safe codec and jitter handling without unbounded buffering.
- [ ] Prove mute, disconnect and deadline behaviour.

Exit criterion: full-duplex canary with packet, memory and lifetime bounds.

## Phase 4 — consumers

- [ ] WebRTC session API and Home Assistant card.
- [ ] Receive-only Opus/AAC stream for SceneTrove.
- [ ] Capability-driven clients; no provider-specific assumptions.

Exit criterion: both consumers pass contract tests while Ring credentials remain
inside the bridge trust boundary.

## Phase 5 — production operations

- [ ] Immutable OCI image, digest-pinned Ansible deployment and rollback.
- [ ] Metrics, alerts, canary separation and redacted structured logs.
- [ ] Rate limits, one-call fanout and account-throttling protection.
- [ ] Operations documentation and recovery drill.

No Home Assistant deployment or restart occurs before Phase 4.
