# Liberty Revival CEF development

- Preserve upstream license and attribution. Keep upstream and LR changes
  identifiable in Git history.
- `origin` is the LR fork. Never push to `upstream`.
- Keep this native repository separate from the game mode and production data.
  No credentials, private keys, player data, caches or local configuration in Git.
- Treat `cef-distribution.json`, the Rust toolchain and dependency commit pins
  as a tested set. Changing one requires compatibility checks.
- Build client/renderer and server from compatible sources; a successful build
  does not demonstrate native audio/render quality.
- Preserve origin/frame restrictions, authenticated transport and generation
  checks. Do not weaken them to facilitate a diagnostic or audio experiment.
- Keep audio experiments scoped. Do not introduce a continuous pilot sound in
  production or bypass GTA gain/pause without an explicit replacement contract.
- Prepare client/render changes separately and obtain approval before installing
  or deploying them. No automatic production deployment from this repository.
- The imported baseline has been source-hash checked. Its audio issue remains
  open; do not label it a successful audio fix.
