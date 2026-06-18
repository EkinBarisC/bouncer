# Defer code-signing for v1; ship unsigned with disclosure

## Context

Bouncer installs global low-level keyboard/mouse hooks. To Windows SmartScreen and AV
heuristics that shape is indistinguishable from spyware, so an **unsigned** binary will show
an "unrecognized app" warning on first run, and may draw AV attention. An Authenticode
certificate (standard OV, or an EV cert for immediate SmartScreen reputation) removes the
warning but costs money annually and requires identity vetting.

## Decision

Ship v1 **unsigned**. Do not buy a certificate now. Instead mitigate trust through:

- **Open source + reproducible build** — the README's "build it yourself" path
  (`cargo build --release`) lets any user produce the same portable exe.
- **SHA256 checksums** published with each GitHub Release so a download can be verified.
- **Explicit disclosure** in the README of the SmartScreen warning and why it appears.

## Considered options

- **Buy an OV code-signing certificate** — removes the warning eventually, but reputation
  builds slowly and the cost isn't justified for an unproven v1.
- **Buy an EV certificate** — instant SmartScreen reputation, but the most expensive option and
  requires a hardware token / vetting; overkill before there is any user base.

## Consequences

- First-run friction (the SmartScreen prompt) is accepted as the cost of an unsigned v1.
- **Revisit trigger:** reconsider signing if SmartScreen friction proves to be a real adoption
  barrier (user reports), or once there is budget and a stable release cadence worth a cert's
  reputation. Until then, the open-source + checksums + disclosure mitigation stands.
