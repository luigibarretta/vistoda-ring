# ADR 0011: Official call recording import

- Status: accepted
- Date: 2026-08-15

## Context

Ring Intercom calls can be answered in Vistoda or the official app. A second
WebRTC recorder started at ding time would compete with the real call, miss the
app user's outbound audio and bypass Ring's official recording notice. Ring
exposes answered call recordings after completion when Call Recording is
enabled and the account is eligible.

## Decision

The bridge accepts a recent ding timestamp and creates one asynchronous import.
It waits 15 seconds before authenticating, polls event history every five
seconds for at most three minutes and accepts only a matching completed event
whose status is `audio_ready` or `ready`. Triggers within five seconds are
deduplicated. One unrelated import may run at a time to prevent refresh-token
races and unbounded vendor work.

The provider-signed media URL must use HTTPS and an allow-listed Ring, AWS or
CloudFront suffix. Download size is capped at 64 MiB and the body must be an MP4
container. The bridge commits media and metadata atomically under a private
directory. Retention is 30 days with a 512 MiB archive ceiling.

List, media, import status and idempotent delete require the bridge bearer
token. Public responses never contain Ring ding identifiers or signed media
URLs. `DELETE` is the consumer acknowledgement contract for SceneTrove.

## Consequences

- the official provider notice and subscription policy remain authoritative;
- app-originated answered calls can be archived without a competing call;
- missed, ineligible or late calls create no local media;
- Home Assistant can trigger imports without holding Ring credentials.
