# Security policy

## Reporting a vulnerability

Please **do not open a public issue** for security problems.

Report them privately through GitHub's private vulnerability reporting:
[Security → Report a vulnerability](https://github.com/trimcraft/trimcraft/security/advisories/new).

Include what you found, how to reproduce it, the Trimcraft version and your operating system version.
Leave out personal data, credentials and tokens. If a log excerpt helps, check it for those
first.

You will get an answer as soon as the maintainer can look at it. Trimcraft is maintained in
spare time, so please allow a few days. Once a fix is released, the advisory is published and
you are credited unless you prefer otherwise.

## Supported versions

Trimcraft is in early access. Only the latest release gets security fixes.

## Scope

Things that matter most:

- secrets (Discord credentials, HTTP header secrets) leaking out of the Windows Credential
  Manager, into files, exports or logs
- the app's IPC commands doing more than the UI needs (opening arbitrary URLs or paths,
  running programs that were not configured)
- the installer, the updater and the release artifacts

Trimcraft never injects into game processes. Reports that a game's anti-cheat flags simulated
input (macros, `SendInput`) are not security issues; see the README.
