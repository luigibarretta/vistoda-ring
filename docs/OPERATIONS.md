# Operations

## Current phase

Version `0.1.x` is a control-plane scaffold. It does not contact Ring and must
not be deployed to Home Assistant. Running it locally proves only configuration,
authentication, fail-closed capability behaviour and offline vendor request
shapes.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `RING_INTERCOM_BIND_HOST` | `0.0.0.0` | listener address |
| `RING_INTERCOM_BIND_PORT` | `8775` | listener port |
| `RING_INTERCOM_API_TOKEN_FILE` | `/run/secrets/api_token` | bearer token file, at least 32 bytes |
| `RING_INTERCOM_DEVICES_FILE` | `/config/devices.json` | alias-only device kinds |

The devices file contains no Ring ID or credential. Only
`ring_intercom_audio` is accepted during the research phase.

## Dedicated Ring session

The research library accepts a separate JSON document with schema version 1,
a stable UUID `hardware_id` and one refresh token. Unknown keys, including a
password, are rejected. On Unix the file must be regular, must not be a symlink
and must have no group or other permissions (normally mode `0600`). Reads are
limited to 16 KiB and secret buffers are zeroed when dropped.

This parser is not wired into the running service yet. No real session should
be created or copied into the repository during Phase 1. Enrolment will be an
explicit operator action in Phase 2 and will never scrape Home Assistant
`.storage`.

## Safe smoke test

1. Create a 32-byte random API token outside Git.
2. Copy `deploy/devices.example.json` to a non-repository runtime directory.
3. Bind to loopback.
4. Query `/healthz` without authentication.
5. Query `/v1/devices` with the bearer token and require `available` to be empty
   and `phase` to be `protocol_research`.

## Failure behaviour

- missing/short token: startup fails;
- missing/invalid devices file: startup fails;
- unsupported device kind: startup fails;
- invalid bearer: `401` with no detail;
- unknown alias: `404`;
- Ring/network outage: impossible in this phase because no provider exists.

No retry, capture or credential enrolment procedure is documented before the
corresponding protocol phase passes review.
