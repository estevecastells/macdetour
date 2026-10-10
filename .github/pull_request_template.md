## What and why

<!-- What does this change, and what problem does it solve? Link the issue. -->

## How it was tested

<!-- Tests added/updated. For network-facing changes, describe the network and ISP you tried it on. -->

## Checklist

- [ ] `make ci` passes locally (fmt, clippy -D warnings, tests, Swift checks, app build)
- [ ] Behaviour changes have tests
- [ ] Protocol changes update `fixtures/` and both the Rust and Swift sides
- [ ] Routes are still never removed while the detour is on, and local/reserved ranges and the server are never detoured
- [ ] No real server addresses, hostnames or keys
