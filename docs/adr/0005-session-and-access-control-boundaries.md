# ADR 0005: Session and access-control boundaries

- Status: accepted
- Date: 2026-08-14

## Context

The existing Home Assistant Ring integration already owns account events and
door unlock. A media experiment with access-control authority would increase
blast radius and could duplicate or race household automations.

## Decision

This bridge never exposes or invokes unlock. Home Assistant retains ding and
unlock ownership. The future bridge uses one dedicated, revocable Ring session
stored in a mode-restricted file. It does not scrape Home Assistant `.storage`
or borrow browser credentials. Authentication enrolment is explicit and never
repeated automatically after rejection.

## Consequences

- compromise of the bridge cannot directly unlock the entrance;
- one additional authorized Ring client may be necessary;
- enrolment and rotation need audited operator workflows;
- loss of the media session leaves supported access automation intact.
