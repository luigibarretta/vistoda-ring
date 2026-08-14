# Operations

## Current phase

Version `0.1.x` is a control-plane scaffold. It does not contact Ring and must
not be deployed to Home Assistant. Running it locally proves only configuration,
authentication and fail-closed capability behaviour.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `RING_INTERCOM_BIND_HOST` | `0.0.0.0` | listener address |
| `RING_INTERCOM_BIND_PORT` | `8775` | listener port |
| `RING_INTERCOM_API_TOKEN_FILE` | `/run/secrets/api_token` | bearer token file, at least 32 bytes |
| `RING_INTERCOM_DEVICES_FILE` | `/config/devices.json` | alias-only device kinds |

The devices file contains no Ring ID or credential. Only
`ring_intercom_audio` is accepted during the research phase.

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
