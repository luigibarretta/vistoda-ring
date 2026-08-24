# ADR 0018: Privacy-safe HTTP failure correlation

- Status: Accepted
- Date: 2026-08-24

## Context

The default Tower HTTP failure event reported status and latency but omitted the
matched route and the bridge error class from Supervisor JSON logs. A production
failure could therefore be detected but not attributed without reproducing it.
Logging raw URIs, request bodies or client correlation headers would expose
aliases, query values or attacker-controlled data.

## Decision

The bridge generates a UUID for every HTTP response and returns it as
`x-request-id`. Failed requests log that server-generated ID, method, normalized
Axum route template, status, latency and a bounded internal error class.

The logger never records raw URIs, query strings, request bodies, authorization
headers, device aliases, enrollment IDs or client-supplied request IDs. Bridge
errors attach non-secret response metadata for the middleware; framework
rejections use a generic bounded class. Successful requests remain silent.

## Consequences

- every HTTP 4xx/5xx can be correlated without exposing provider state;
- the route label has bounded cardinality and cannot contain user input;
- internal failures distinguish I/O, transport, protocol, provider,
  configuration and serialization classes;
- clients may include the response request ID in a support report.
