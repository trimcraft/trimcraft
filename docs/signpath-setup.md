# Setting up Windows code signing (SignPath Foundation)

Maintainer notes. Trimcraft signs its Windows releases through the free open source program of
the [SignPath Foundation](https://signpath.org). The repository side is prepared: the
[code signing policy](../README.md#code-signing-policy), the release workflow
([release.yml](../.github/workflows/release.yml), off until `SIGNPATH_ENABLED` is `true`) and
the SignPath configuration in [`.signpath/`](../.signpath/README.md). This page lists what to do
in SignPath and GitHub once the application is approved.

## Before applying

- Two-factor authentication on the GitHub account of every maintainer, and required for the
  organisation: github.com/organizations/trimcraft/settings/security → "Require two-factor
  authentication". SignPath requires MFA for repository and SignPath access.
- Make your membership of the `trimcraft` organisation public
  (github.com/orgs/trimcraft/people → your row → Public), so the "members" link in the code
  signing policy shows the committers.
- The code signing policy is on `main` (README, section "Code signing policy") and the release
  notes link to it.
- At least one published release on GitHub Releases (v0.1.0), in the form that will be signed.

## 1. Account and organisation

1. Accept the invitation from SignPath and sign in at <https://app.signpath.io>.
2. Turn on multi-factor authentication for your SignPath user (user menu → profile/security).
3. Note the **organisation ID**: Organization → Settings (a GUID, also in the URL
   `app.signpath.io/<organization-id>/...`).

If SignPath has already created the project or policies for you, check them against the steps
below instead of creating new ones.

## 2. Trusted build system

Organization → Trusted Build Systems → **Add** → predefined **GitHub.com**. This is what lets
SignPath verify that a request comes from a GitHub Actions run of this repository (origin
verification). The SignPath GitHub App is not needed for a public repository.

## 3. Project

Projects → **Add**:

| Field | Value |
|---|---|
| Name | Trimcraft |
| Slug | `trimcraft` |
| Repository URL | `https://github.com/trimcraft/trimcraft` |

In the project, link the trusted build system **GitHub.com** (Trusted build systems → Add).

## 4. Artifact configurations

Project → Artifact configurations → **Add** → "Custom XML", twice:

| Slug | Paste |
|---|---|
| `trimcraft-exe` | [`.signpath/artifact-configurations/trimcraft-exe.xml`](../.signpath/artifact-configurations/trimcraft-exe.xml) |
| `installer` | [`.signpath/artifact-configurations/installer.xml`](../.signpath/artifact-configurations/installer.xml) |

Neither needs to be the default; the workflow names the slug in each request.

## 5. CI user and API token

1. Users → **Add CI user**, e.g. `github-actions-trimcraft`. Create its **API token** and copy it.
2. Store it in the repository (PowerShell or bash, from any folder):

   ```bash
   gh secret set SIGNPATH_API_TOKEN --repo trimcraft/trimcraft
   ```

   (`gh` asks for the value; paste the token.)

## 6. Signing policy

Project → Signing policies → **Add**:

| Field | Value |
|---|---|
| Name / slug | Release signing / `release-signing` |
| Purpose | Release signing |
| Certificate | the SignPath Foundation certificate assigned to the project |
| Submitters | the CI user from step 5 (only) |
| Use approval process | on; Approvers: you; required approvals: 1 |
| Trusted build system verification | on |
| Verify origin | on |
| Allowed branch names | `refs/tags/v*` (see below) |

The workflow submits only from runs of a `v*` tag. SignPath does not document how a tag build's
ref appears to the branch check. If the first test release is denied because of the branch,
open the denied request, read the branch/ref SignPath reports, and set the allowed name to the
same form (e.g. `v*`).

**Pipeline policy:** Project → the policy → Pipeline policies → **Add**, level **Enforce**,
with the YAML from
[`.signpath/policies/trimcraft/release-signing.yml`](../.signpath/policies/trimcraft/release-signing.yml)
(external definition from this repository, or pasted as an internal definition). It requires
GitHub-hosted runners.

Every signing request needs your approval: SignPath sends an e-mail per request. A release sends
**two** requests (program, then installer), and the workflow waits up to an hour for each.

## 7. Repository variables

```bash
gh variable set SIGNPATH_ORGANIZATION_ID --repo trimcraft/trimcraft --body "<organisation id>"
```

```bash
gh variable set SIGNPATH_PROJECT_SLUG --repo trimcraft/trimcraft --body "trimcraft"
```

```bash
gh variable set SIGNPATH_SIGNING_POLICY_SLUG --repo trimcraft/trimcraft --body "release-signing"
```

```bash
gh variable set SIGNPATH_APP_CONFIGURATION_SLUG --repo trimcraft/trimcraft --body "trimcraft-exe"
```

```bash
gh variable set SIGNPATH_INSTALLER_CONFIGURATION_SLUG --repo trimcraft/trimcraft --body "installer"
```

Then switch signing on:

```bash
gh variable set SIGNPATH_ENABLED --repo trimcraft/trimcraft --body "true"
```

`SIGNPATH_ENABLED` set to anything else (or deleted) brings back the unsigned release path
exactly as before.

## 8. Dry run

Actions → Release → **Run workflow** (on `main`). With signing on, it goes through the split
build (program, installer, updater signature, `latest.json`) without the SignPath requests,
which only accept tag builds, and keeps the files as the artifact `trimcraft-release-dry-run`.
Check that it is green and the artifact holds the installer, its `.sig` and `latest.json`.

## 9. Test release v0.1.1

Follow "Releasing" in [CLAUDE.md](../CLAUDE.md): bump the version to 0.1.1 in the four files,
add the CHANGELOG section, run `./scripts/check.sh`, commit, merge to `main`, then

```bash
git tag v0.1.1
```

```bash
git push origin v0.1.1
```

Watch the run (Actions → Release). Approve the request for `trimcraft-exe` in SignPath (e-mail
link), then the one for `installer` a few minutes later. After that the workflow signs the
updater package and publishes "Trimcraft v0.1.1 (early access)".

## 10. Verify

Download `Trimcraft_0.1.1_x64-setup.exe` from the release page.

```powershell
Get-AuthenticodeSignature .\Trimcraft_0.1.1_x64-setup.exe | Format-List Status, StatusMessage, SignerCertificate, TimeStamperCertificate
```

`Status` must be `Valid`, the signer `SignPath Foundation`, with a timestamp. In Explorer:
right-click the file → Properties → **Digital Signatures** → the entry → Details → "This digital
signature is OK."

Install it, then check the program the same way:

```powershell
Get-AuthenticodeSignature "$env:LOCALAPPDATA\Trimcraft\trimcraft.exe" | Format-List Status, SignerCertificate
```

And the updater: an installed v0.1.0 shows the **update** badge, and **Download & install**
under Settings → Updates installs v0.1.1 (this checks the `.sig` from the signed installer).

SmartScreen may still warn for the first downloads: its reputation builds up per certificate
over time. Windows' security prompts show "SignPath Foundation" as the publisher instead of
"Unknown publisher" at once.

## Afterwards

- In the README, drop the note that Trimcraft "has applied" and name the first signed version.
- Each release: approve the two signing requests while the workflow waits (an hour each). If one
  times out, re-run the failed job (the pipeline policy allows re-runs).
- Only you (`@bresam`) approve. If another maintainer joins, add them to the code signing
  policy's team roles first, and to SignPath with MFA.
