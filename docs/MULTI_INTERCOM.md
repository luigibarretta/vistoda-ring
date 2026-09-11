# Multiple Ring intercoms

The Home Assistant app discovers up to 32 entrances under one enrolled Ring
account. Each routed alias binds to a different positive Ring device ID.
Commands, events, audio sessions and
recordings use that exact binding; missing or ambiguous identities fail closed.

## Home Assistant app

Leave `intercoms: []` for normal setup, even with several entrances. Install and
start the app, complete Ring login/SMS in the discovered Home Assistant flow,
then select the real intercom by its name/location. No numeric ID copy is needed.
The provider discovers the account inventory and assigns stable `intercom-<id>`
aliases; Home Assistant offers the remaining entrances as follow-up discovery.
Select the desired entrance in Vistoda before opening or viewing its history.

Advanced installations may provide `{alias, device_id}` objects in `intercoms`
to preserve chosen aliases for known devices. This overrides their generated
aliases, not account access; other discovered devices can still be offered:

```yaml
alias: entrance
intercoms:
  - alias: entrance
    device_id: 100000001
  - alias: side-gate
    device_id: 100000002
recording_storage: private
```

Replace example IDs with verified Ring device identities. An account/location
ID or MAC address is not a device ID. Keep aliases stable and select the desired
entrance in Vistoda before any action. The top-level `alias` is only a discovery
preference when a list is configured; it does not add another implicit entrance.
App IDs are limited to JavaScript's exact integer range to avoid rounding during
configuration and discovery. The standalone Rust configuration accepts `u64`.

Supervisor stores one discovery message per app/service. After health succeeds,
bootstrap reads the authenticated, bounded inventory and publishes only its
`aliases` and string-valued `device_id` bindings, never account names/locations.
Before enrollment (or if inventory is unavailable), it publishes static bootstrap
configuration; HA continues with fresh inventory after login. A later app restart
discovers the enrolled routes again. The legacy `alias` field remains present.
Every entry shares the private URL/token and account session, not device state.

## Standalone container

`deploy/devices.example.json` shows the equivalent explicit Rust configuration.
Each object maps an alias to `kind: ring_intercom_audio` and `device_id`. A single
legacy object may omit `device_id`; multiple configured aliases must all be bound.
Enrollment and authenticated `GET /v1/intercoms` also provision discovered routes.
The provider rejects duplicate bindings and alias collisions without rebinding.

## Archives and migration

Explicitly bound devices store files under `<archive>/device-<id>`. Renaming an
alias therefore preserves its recordings. Reassigning an alias to a different
physical ID does not expose the previous device's archive. Removing a configured
device keeps its directory on disk; restoring the same binding restores access.

Existing unbound single-device routes retain their flat archive. Older manifests have
no physical device identity, so switching to explicit bindings preserves those
files in place without attributing them to an entrance. Export wanted files
before switching and keep an independently verified backup.

Changing storage destination copies all recognized root files and device
directories, verifies their contents, then retires only matching source files.
All directory copies finish before source removal begins. A later conflict
cannot remove an earlier device's source archive. Symlinks, unknown directories,
unexpected file names and mismatched bytes fail closed. Partial copies can remain
at the target after failure; the source and previous storage marker are retained
so an operator can resolve the exact conflict and retry.

## Più ingressi

Lascia `intercoms` vuoto, completa login/SMS e scegli il citofono dal suo nome e
posizione: gli alias `intercom-<id>` sono creati automaticamente, senza copiare ID.
Gli altri ingressi vengono proposti da Home Assistant. La lista manuale resta
un'opzione avanzata per conservare alias personalizzati con ID verificati.
Seleziona il citofono in Vistoda prima di aprire, consultare eventi o usare audio.
Ogni archivio bound vive in `device-<id>`; i vecchi file senza identità rimangono
nella radice ed è opportuno esportarli prima del passaggio. Una migrazione di
storage verifica tutte le copie prima di ritirare i sorgenti e si ferma sui conflitti.
