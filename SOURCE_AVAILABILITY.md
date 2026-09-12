# Covered source included with Vistoda Ring

The normal dependency graph includes `ece 2.3.1` under MPL-2.0, through
`fcm-push-listener-ring`. Vistoda's own license does not replace that license.

Both container variants include the complete build-time `ece` source tree at:

`/usr/share/doc/vistoda/dependencies/sources/ece-2.3.1.tar.gz`

The adjacent `SHA256SUMS` verifies the archive; its source tree includes the MPL
license. You may obtain and modify this covered source under those terms.
Extract it with `tar -xzf ece-2.3.1.tar.gz`. The archive is copied from the source
used for this build, not downloaded later from a moving upstream branch.
The packaging step fails if the reviewed version is no longer in the dependency
graph. Build distributors must preserve this file and the archive when repackaging
or providing a standalone executable. These changes do not retroactively repair
already published images; those require inspection and a replacement release.

The MIT-licensed listener fork and its modification notes are in
`vendor/fcm-push-listener-ring`. Other Cargo notices are under `dependencies`.
Upstream ece package: https://crates.io/crates/ece/2.3.1

This notice covers the identified MPL component, not a certification that all
operating-system packages or every distribution channel have been audited.
