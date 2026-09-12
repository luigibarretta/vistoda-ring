# Protocol provenance follow-up — 2026-09-12

This is an evidence inventory, not a declaration of vendor approval or a
certified clean-room process. Existing functional account/device controls remain
unchanged by the documentation and distribution hardening.

- `docs/RESEARCH.md` records ring-client-api commit
  `638d5285aea5f34d44d9bacbb41917f736764d49e`. Some historical links still point
  to `main`; a current tip must not be represented as the historical revision
  actually studied. Recover the historical pin or explicitly retain that gap.
- `vendor/fcm-push-listener-ring/VISTODA.md` records the 4.0.3 fork and local
  changes. Its MIT license is retained and copied into both image variants.
- Official-app client identifiers and Firebase project identifiers are protocol
  compatibility inputs, not proof of authorization or affiliation. Their use
  needs an approved contract or case-specific legal assessment.
- `SOURCE_AVAILABILITY.md` documents the bundled MPL-covered `ece` source and
  exact recipient instructions. A dependency bump deliberately requires review
  of the source-packaging gate.

Before legal clearance, map individual private API/signaling behaviors to pinned
public sources or dated observations, verify applicable account terms, and
document acquisition/method/necessity for any proprietary-app analysis. Do not
invent missing research history or include live credentials in the dossier.
