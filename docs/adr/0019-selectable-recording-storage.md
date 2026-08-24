# ADR 0019: Selectable and observable recording storage

- Status: accepted
- Date: 2026-08-24

## Context

Vistoda Ring originally kept every local call recording in the app-private
`/data/recordings` directory. That default is safe and backup-friendly, but it
does not let a Home Assistant OS user make the archive directly visible in an
app configuration, media or share directory. The panel also could not state
where a listed recording was stored.

## Decision

The bridge accepts separate runtime and display directories plus a bounded
storage kind. Its authenticated recording inventory returns the effective
storage descriptor and an exact display path for every media item. It never
returns credentials, bridge URLs or provider identifiers.

The managed HAOS app offers only four mounted destinations: private app data,
public app configuration, Home Assistant media and Home Assistant share. It
records the active choice and migrates generated archive files between those
known roots after copying and comparing every file. A conflict fails closed;
the source is removed only after the complete copy verifies.

Arbitrary host paths are deliberately unsupported. Standalone operators may
still configure a custom runtime and display path through environment variables.

## Consequences

Private storage remains the upgrade-compatible default. Public app-config data
is included with app backups; media and share are user-visible but need their
own backup policy. The Home Assistant panel can show one non-redundant archive
summary and disclose each exact file path on demand.
