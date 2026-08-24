# ADR 0020: Fail-closed HAOS network recording storage

- Status: accepted
- Date: 2026-08-24

## Context

ADR 0019 added `/media/vistoda-ring` and `/share/vistoda-ring`, but those
directories are local HAOS storage unless Supervisor has mounted network
storage at that exact path. Calling the `share` destination a Samba share was
therefore ambiguous. Home Assistant OS can manage NFS and CIFS storage and
bind a named mount below `/media` or `/share` for apps.

The app must also avoid writing into a local lookalike directory when a NAS is
unavailable. That failure would split one archive across local and remote
storage without telling the operator.

## Decision

The managed app adds an explicit `network` choice plus a bounded mount-root
option. Accepted roots have exactly the form `/media/<name>` or
`/share/<name>`; nested paths, traversal and other host paths are rejected.
Vistoda appends its own `vistoda-ring` directory.

Before migration or startup, the app requires the root to exist and have a
different filesystem device from its local parent. This mirrors Supervisor's
network-mount liveness boundary and prevents a missing mount from degrading to
local writes. The marker stores both the network choice and root so a later
migration cannot silently abandon the remote source.

The public recording contract continues to report `media` or `share`, derived
from the HAOS usage path. It does not expose transport credentials or invent a
provider-specific storage kind.

## Consequences

NFS and Samba credentials remain owned by Supervisor. The Vistoda app gains no
manager role and cannot enumerate or create mounts. Users first add network
storage in Home Assistant, then copy its resulting root into the app option.

If the selected mount is missing, startup stops before migration or recording.
The archive directory must be writable by the non-root Vistoda UID 10001; NAS
exports retain responsibility for their own permissions and backup policy.
