# Vistoda Ring release state

This document records the release gates for Vistoda Ring. It is not a roadmap
promise and does not expand the supported surface described in the repository
README and OpenAPI contract.

## Released in 0.13.0

- dedicated password/SMS enrollment with private rotating session storage;
- discovery and explicit physical binding for one or more Ring Intercoms;
- native status, volume, one-shot unlock and paginated event history;
- bounded WebRTC audio with microphone permission controlled by the browser;
- native push-event cursor with redacted metrics and logs;
- private local call recordings with selectable HAOS storage;
- authenticated Home Assistant proxying without browser-visible provider tokens;
- rootless, read-only multi-architecture images and signed release artifacts.

## Added in 0.14

- native Ring doorbell and camera discovery with signaling-only live sessions;
  media flows directly between the browser and Ring
  ([native cameras](NATIVE_CAMERAS.md));
- rotation of stale native push registrations.

The owned-device release evidence covered enrollment, discovery, inbound and
outbound PCMU, mute, session teardown, event history and exact device routing.
Deterministic tests cover failure and privacy boundaries without contacting Ring
or operating an entrance.

## Release gates

A Ring release must pass all of these gates:

1. Rust formatting, strict Clippy, locked tests and dependency audit.
2. OpenAPI and consumer contract tests for every changed endpoint.
3. Container startup as the unprivileged runtime user with a read-only root.
4. No credential, session, SDP, ICE, device identity or event payload in logs.
5. Exact alias-to-device routing for controls, history, audio and recordings.
6. Idempotent teardown and no automatic retry of a door action.
7. Signed `amd64` and `aarch64` images built from the tagged source commit.
8. Matching Vistoda app metadata and compatibility documentation.

Real-device canaries are explicit and bounded. They stop on account warnings,
provider throttling, HTTP 401/403/429 or unexpected physical behavior. Door
opening is never used as a routine release test.

## Known boundary

Ring supports IFTTT and Alexa as its official third-party paths. Vistoda Ring
uses experimental consumer APIs and is not endorsed or supported by Ring. A
vendor change can interrupt enrollment, events, controls or media.

Native push requires outbound HTTPS and TCP 5228 to `mtalk.google.com`.
Microphone quality still depends on the browser, client hardware, network path
and the acoustic environment. The provider does not claim Ring cloud recording.
Native camera live video is experimental: NAT and firewall compatibility of the
direct browser-to-Ring path still needs per-device acceptance.

## Future changes

New protocol work starts with captured owner-authorized evidence, a redacted
fixture and an explicit ADR. It remains hidden from consumers until the exact
device path, recovery behavior, resource bounds and teardown have passed both
deterministic tests and a deliberate owned-device canary.
