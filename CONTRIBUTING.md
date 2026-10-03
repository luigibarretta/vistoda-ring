# Contributing

Read the Vistoda family [contribution guide](https://github.com/luigibarretta/vistoda-home-assistant/blob/main/CONTRIBUTING.md)
before changing a cross-repository contract.

This repository owns Ring enrollment, intercom identity, controls, events and
audio transport. The Home Assistant panel belongs in `vistoda-home-assistant`;
app-store metadata belongs in `vistoda-addons`.

Run the validation commands in [README.md](README.md#development). Real-device
tests are opt-in, owner-authorized and never use door opening as a smoke test.
Report security issues through [SECURITY.md](SECURITY.md), not a public issue.
