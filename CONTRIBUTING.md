# Contributing to Trimcraft

Thanks for helping. Trimcraft is in early access and maintained in spare time, so feedback from
real cockpits is the most valuable contribution: bugs, rough edges and ideas.

## Issues

- **Bugs:** use the [bug report](https://github.com/trimcraft/trimcraft/issues/new?template=bug_report.yml)
  template. Include your Windows and Trimcraft versions, the devices involved and a log
  excerpt (Hardware Test → Event log, or `%LOCALAPPDATA%\cloud.breu.trimcraft\logs\trimcraft.log`).
  Check the excerpt for names, paths or anything private before posting.
- **Ideas:** use the [feature request](https://github.com/trimcraft/trimcraft/issues/new?template=feature_request.yml)
  template and describe the situation in the game or sim, not only the solution.
- **Security problems:** never in a public issue; see [SECURITY.md](SECURITY.md).

## Pull requests

Open an issue first for anything bigger than a small fix, so we can agree on the approach.

Ground rules (details in [CLAUDE.md](CLAUDE.md), which is the working guide for everyone,
human or agent):

- Code, comments, commit messages and docs in English.
- No special cases for a specific controller vendor or model. The only device id the code
  knows is vJoy's.
- Input is event-driven: every edge goes through `Engine::apply`; never sample per frame.
- Logic lives in `trimcraft-core` (no OS dependencies, unit-tested); the Tauri crate wires
  things together.
- Never inject into game processes. The overlay is a separate window.
- When you change a serialised Rust type, update `ui/src/lib/types.ts`, the browser demo
  (`ui/src/lib/demo.ts`) and the payload test in `src-tauri/src/commands.rs`.
- Run `./scripts/check.sh` before pushing; CI runs the same gate.
- When dependencies change, regenerate `THIRD_PARTY_LICENSES` with
  `./scripts/third-party-licenses.sh`.

See the README's "Build from source" section for the toolchain.

## Licence of contributions

Trimcraft is licensed under the GNU General Public License v3.0 or later. By submitting a
contribution you agree that it is licensed under the same terms (inbound = outbound). There is
no contributor licence agreement.

## Conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
