# Windows Package Manager (winget)

Maintainer guide for publishing Trimcraft to the
[winget community repository](https://github.com/microsoft/winget-pkgs), so users can run:

```powershell
winget install Trimcraft.Trimcraft
```

## Package

| | |
|---|---|
| Identifier | `Trimcraft.Trimcraft` (`Publisher.Package`, case-sensitive in the repository path) |
| Moniker | `trimcraft` (so `winget install trimcraft` works too) |
| Manifests | `manifests/t/Trimcraft/Trimcraft/<version>/` in winget-pkgs; the reference copy for the first version is [`packaging/winget/`](../packaging/winget) |
| Manifest format | multi-file (version, defaultLocale `en-US`, installer), schema 1.12.0 (what winget-pkgs uses; singleton manifests are not allowed there) |
| Installer | `Trimcraft_<version>_x64-setup.exe` from the GitHub release, `InstallerType: nullsoft`, `Scope: user` |

What the NSIS installer from Tauri writes, and what the manifest relies on:

- It installs per user (`installMode: currentUser`) to `%LOCALAPPDATA%\Trimcraft` without
  administrator rights, and registers under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Trimcraft` with `DisplayName`
  `Trimcraft`, `DisplayVersion` `<version>` and `Publisher` `Trimcraft` (`productName`, `version`
  and `bundle.publisher` in `tauri.conf.json`). The manifest names the key as
  `ProductCode: Trimcraft` and lists the same values in `AppsAndFeaturesEntries`, so winget
  recognises existing installations, including ones installed from the website. The key name
  does not depend on the publisher, so installations of versions before 0.1.4, which wrote a
  different publisher, are matched by the product code as well.
- winget's defaults for `nullsoft` fit: `/S` for silent installs (the installer does not start
  Trimcraft afterwards without `/R`), `/D=<path>` for a custom location. No custom switches.
- `UpgradeBehavior: install`: the new installer runs over the existing installation, as the
  installer from the website does.
- winget and Trimcraft's own updater work side by side: both install the same per-user package,
  and winget reads the installed version from the registry.

The installer is not code-signed. winget-pkgs does not require signed installers; the validation
pipeline scans every installer (several antivirus engines, an unattended install in a clean VM,
Defender after installation) and checks URL reputation instead. winget verifies the SHA256 of the
download against the manifest.

## Automatic updates (`.github/workflows/winget.yml`)

After every release, the WinGet workflow opens the pull request for the new version in
winget-pkgs with [WinGet Releaser](https://github.com/vedantmgoyal9/winget-releaser) (which runs
[Komac](https://github.com/russellbanks/Komac)). It takes the latest version in winget-pkgs as the
base and updates the installer URL, SHA256, release date and release notes.

- **Trigger:** a successful Release run for a tag (`workflow_run`). The Release workflow publishes
  with `GITHUB_TOKEN`, and events caused by that token start no other workflows, so
  `release: published` would never fire for our own releases; it stays as a trigger for releases
  published by hand. A manual Release run (dry run) submits nothing.
- **By hand:** Actions → WinGet → "Run workflow" with a tag (for example to resubmit after a
  failure), or `gh workflow run winget.yml -R trimcraft/trimcraft -f tag=v0.1.5`.
- **Without the secret** `WINGET_TOKEN`, the job ends successfully with a notice and submits
  nothing.
- WinGet Releaser **only updates existing packages**. Until the first version is merged in
  winget-pkgs (below), the job fails; that does not affect the release itself.

The action is pinned to a commit; Dependabot (`github-actions`) keeps it current. Note that the
action itself installs `cargo-binstall` from its `main` branch and the latest Komac release, which
the pin does not cover. The job only gets read access to this repository; the token it can misuse
is `WINGET_TOKEN` (`public_repo`).

## One-time setup

### 1. Token for the workflow (`WINGET_TOKEN`)

WinGet Releaser needs a **classic** personal access token (fine-grained tokens are not
supported) of the account that owns the winget-pkgs fork. The pull requests come from that
account; the workflow uses `bresam` (`FORK_USER` in `winget.yml`).

1. GitHub (logged in as `bresam`) → Settings → Developer settings → Personal access tokens →
   **Tokens (classic)** → Generate new token (classic)
   (<https://github.com/settings/tokens/new>).
2. Note: `trimcraft winget-releaser`. Expiration: 1 year (GitHub warns by email before it
   expires; then repeat these steps).
3. Scopes: **`public_repo`** only. Optionally also `workflow`: WinGet Releaser syncs the fork with
   upstream, which can fail when upstream changed workflow files; without the scope, sync the fork
   by hand on GitHub (Sync fork) and rerun the job.
4. Generate, copy the token and store it as repository secret (it prompts for the value, so the
   token does not end up in the shell history):

   ```sh
   gh secret set WINGET_TOKEN -R trimcraft/trimcraft
   ```

5. The fork `bresam/winget-pkgs` must exist. The first submission with `wingetcreate` (below)
   creates it; otherwise fork <https://github.com/microsoft/winget-pkgs> on GitHub.

### 2. First submission (manual)

Submitting is a public pull request to microsoft/winget-pkgs under your account. It needs a
signed [Microsoft CLA](https://cla.opensource.microsoft.com/microsoft/winget-pkgs) (the bot asks
on the pull request) and one package version per pull request.

Submit the **latest** published version. If that is no longer the version in
`packaging/winget/`, update the manifests first (see [New version by hand](#new-version-by-hand)).

1. Check that nothing exists yet:

   ```powershell
   winget search Trimcraft
   ```

   and that there is no open pull request:
   <https://github.com/microsoft/winget-pkgs/pulls?q=Trimcraft>.

2. Validate:

   ```powershell
   winget validate --manifest packaging\winget\manifests\t\Trimcraft\Trimcraft\0.1.4
   ```

3. Optional, test the installation. This installs Trimcraft for your user like the installer
   from the website (over an existing installation, as an update). Enabling local manifests needs
   an administrator terminal once:

   ```powershell
   winget settings --enable LocalManifestFiles
   winget install --manifest packaging\winget\manifests\t\Trimcraft\Trimcraft\0.1.4
   ```

   For an isolated test, use Windows Sandbox with winget-pkgs'
   [`Tools\SandboxTest.ps1`](https://github.com/microsoft/winget-pkgs/blob/master/doc/tools/SandboxTest.md).

4. Submit with [wingetcreate](https://github.com/microsoft/winget-create)
   (`winget install wingetcreate`). Without `--token` it signs in to GitHub in the browser, forks
   winget-pkgs under your account and opens the pull request:

   ```powershell
   wingetcreate submit --prtitle "New package: Trimcraft.Trimcraft version 0.1.4" packaging\winget\manifests\t\Trimcraft\Trimcraft\0.1.4
   ```

   By hand instead: fork microsoft/winget-pkgs, copy the folder to
   `manifests/t/Trimcraft/Trimcraft/0.1.4/` (only these three files), commit, open a pull request
   titled `New package: Trimcraft.Trimcraft version 0.1.4` and fill in the checklist.

5. Watch the pull request: the validation bot sets labels (`Validation-Completed` when it
   passes, otherwise see the
   [failure guide](https://github.com/microsoft/winget-pkgs/blob/master/doc/ValidationFailureGuide.md)),
   then a moderator reviews it. A new package usually takes a few days.

From the next release on, the workflow submits each version.

### New version by hand

Only needed when the workflow cannot run (or to refresh `packaging/winget/`). Copy the version
folder, rename it to the new version and change in the three files: `PackageVersion`; in the
installer manifest `InstallerUrl`, `InstallerSha256` and `ReleaseDate`; in the locale manifest
`ReleaseNotesUrl`. The SHA256 (uppercase in the manifest) comes from the release:

```sh
gh api repos/trimcraft/trimcraft/releases/tags/v0.1.5 --jq '.assets[] | select(.name | endswith("-setup.exe")) | .digest'
```

Then `winget validate --manifest <folder>`. With wingetcreate, `wingetcreate update
Trimcraft.Trimcraft --version <version> --urls <installer URL>` does the same once the package
exists in winget-pkgs.

## When something changes

- **Publisher name:** changing `bundle.publisher` in `tauri.conf.json` changes the `Publisher` the
  installer writes (and Tauri's `HKCU\Software\<publisher>` key). Update `AppsAndFeaturesEntries`
  in the next manifest accordingly (Komac reads it from the installer).
- **Install mode, product name or installer file name:** check `Scope`, `ProductCode` and the
  `installers-regex` in `winget.yml`.
- **Another architecture** (e.g. arm64): add an installer entry; the workflow's regex must match
  the new file.
