# Contributing

Thanks for helping. Issues and PRs are welcome.

## Setup

```bash
make ci        # fmt, clippy -D warnings, Rust tests, Swift protocol checks, app build
make probe     # see which listed addresses are blocked on your network (changes nothing)
```

## Guidelines

- Keep `detour-core` free of OS calls; put anything that touches the system behind the `Platform` trait in `detourd` so the engine stays testable with a fake.
- Behaviour changes need tests. Protocol changes update `fixtures/` and both the Rust and Swift sides.
- Never remove a detour route while the detour is on (that would cut live connections), and never detour local or reserved ranges or the tunnel server.
- No new dependencies without a reason in the PR description.
- Never commit real server addresses, hostnames or keys. Use documentation-style placeholders.
