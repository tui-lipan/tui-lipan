# Security Policy

## Reporting a Vulnerability

If you believe you have found a security vulnerability in **tui-lipan**, please
**do not** open a public GitHub issue. Instead, report it privately so the
issue can be triaged and patched before disclosure.

**Email:** [security@tui-lipan.dev](mailto:security@tui-lipan.dev)

Please include:

- A description of the vulnerability and its potential impact
- Steps to reproduce (a minimal example is ideal)
- The version of `tui-lipan` (and enabled features) where you observed it
- Any suggested mitigations, if you have them

You can expect:

- An acknowledgement within **72 hours**
- A first assessment and triage within **7 days**
- A fix or mitigation plan communicated within **30 days** for confirmed issues
- Credit in the release notes (unless you prefer to remain anonymous)

## Scope

This policy covers the `tui-lipan` and `tui-lipan-macro` crates published from
this repository.

In-scope examples of issues we want to hear about:

- Memory safety issues exposed via the public API (UAF, OOB reads/writes)
- Denial-of-service via crafted input (e.g. inputs to `TextArea`, `DocumentView`,
  the markdown formatter, the syntax highlighter, or the embedded terminal)
- Issues in feature-gated dependencies where `tui-lipan` exposes them in a way
  that bypasses their intended usage

Out of scope:

- Issues in upstream crates that don't manifest through `tui-lipan`'s API -
  please report those to the upstream project directly
- Theoretical issues without a working proof-of-concept

## Supported Versions

While `tui-lipan` is on `0.x.y`, security fixes are released against the latest
minor version only. Once `1.0.0` lands, this policy will be updated to cover
back-porting against supported branches.

## GPG / Signed Reports

If you would like to encrypt your report, mention this in your initial email
and we will exchange a public key.
