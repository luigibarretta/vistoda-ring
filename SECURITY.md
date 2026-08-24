# Security policy

## Supported versions

Only the latest tagged minor release receives security fixes. The protocol
research branch is never suitable for Internet exposure.

## Reporting

Report vulnerabilities privately to the repository owner. Do not open a public
issue containing credentials, Ring identifiers, SDP, ICE candidates, packet
captures, household audio, internal addresses or authentication responses.

## Research rules

- use only accounts and devices owned by the operator;
- do not bypass account protections or certificate validation in production;
- do not brute-force credentials, identifiers or undocumented commands;
- redact authorization, cookies, refresh tokens, SDP and signed endpoints;
- keep raw captures outside Git with restrictive permissions and bounded
  retention;
- stop immediately on account lockout, throttling or unexpected device action.

## Door control

As accepted in ADR 0012, the bridge owns a bearer-authenticated native unlock
endpoint for an enrolled alias. It validates alias and authorization before
provider access, issues one vendor command and performs no retry above the
client's single authentication-refresh retry. Consumers must require an
explicit destructive confirmation and must not retry an ambiguous result.

Home Assistant may retain its supported Ring integration only as an explicit
fallback. It must suppress that fallback whenever the native path may already
have sent the command, preventing a possible double unlock.
