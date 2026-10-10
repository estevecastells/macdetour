# Releasing

Releases are built and published by `.github/workflows/release.yml` when a version tag is pushed.

1. Bump `version` in the root `Cargo.toml` (for example `0.2.0`), then run `cargo build` to refresh `Cargo.lock`.
2. If the UI changed, refresh the README images: `(cd app && swift run RenderScreenshots ../docs/images)`.
3. Check that `make ci` passes, then commit: `Release v0.2.0`.
4. Tag and push: `git tag v0.2.0 && git push origin main v0.2.0`
5. The workflow checks that the tag matches the Cargo version, runs the tests, builds and smoke-tests the
   artifacts, and publishes a GitHub release with:
   - `MacDetour-macos-arm64.zip`: menu bar app, bundling the daemon, CLI and installer
   - `macdetour-macos-arm64.tar.gz`: `detourd`, `detourctl` and the install scripts
   - `SHA256SUMS`
   - `SHA256SUMS.sig`: Ed25519 signature checked by the in-app updater, made with the `UPDATE_SIGNING_KEY`
     secret. The workflow fails rather than publish an unsigned release.
6. Verify: `MACDETOUR_DOWNLOAD_ONLY=1 bash scripts/get.sh` downloads the latest release and checks its checksums.

Installed apps pick the release up within six hours (or with **Buscar actualizaciones**), replace themselves,
and then offer to update the service, which asks for the administrator password.

Asset names carry no version, so `releases/latest/download/<asset>` always resolves to the newest release.

## Signing key

`swift scripts/sign-update.swift --generate` prints a new key pair. The private key goes in the
`UPDATE_SIGNING_KEY` repository secret (keep an offline backup); the public key goes in
`UpdateConfig.publicKey`. Rotating it means older apps can no longer update themselves, so avoid it.
