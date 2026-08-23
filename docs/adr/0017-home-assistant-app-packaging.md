# ADR 0017: Home Assistant app packaging

- Status: Accepted
- Date: 2026-08-23

## Context

The standalone Ring runtime is appropriate for SceneTrove and native clients,
but requiring a separate Docker host, URL and workload token makes ordinary
Home Assistant installation unnecessarily operational.

## Decision

The same Rust core is distributed in two packages:

- the existing standalone OCI image for remote and multi-consumer deployments;
- a Home Assistant app image discovered through the private Supervisor API.

The app generates its workload token inside `/data`, keeps port 8775 private,
publishes the token only in a Supervisor discovery message and asks the Vistoda
config flow to perform Ring enrollment. The app wrapper contains no Ring
protocol logic. `vistoda-addons` owns store metadata and the provider repository
owns the executable image.

## Consequences

Home Assistant users never enter bridge networking or authentication values.
Remote deployments remain supported without forking the Rust core. Release
images must publish matching `amd64` and `aarch64` manifests, and app metadata
cannot advance until both exist.

