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

The bridge never owns door unlock. That remains with Home Assistant's supported
Ring integration and its existing confirmation automation.
