# SignPath configuration

Windows code signing for Trimcraft goes through the [SignPath Foundation](https://signpath.org)
([code signing policy](../README.md#code-signing-policy)). This folder holds what SignPath needs
from the repository. The step-by-step setup is in [docs/signpath-setup.md](../docs/signpath-setup.md).

| File | Goes to (SignPath web app) |
|---|---|
| `artifact-configurations/trimcraft-exe.xml` | Project `trimcraft` → Artifact configurations → New, slug `trimcraft-exe`, "Custom XML": paste the file |
| `artifact-configurations/installer.xml` | Project `trimcraft` → Artifact configurations → New, slug `installer`, "Custom XML": paste the file |
| `policies/trimcraft/release-signing.yml` | Pipeline policy for the signing policy `release-signing` (external definition from this repository, or paste the YAML as an internal definition) |

The release workflow ([release.yml](../.github/workflows/release.yml)) sends two signing requests
per tagged release, in this order:

1. `trimcraft-exe`: a zip with `trimcraft.exe` (the GitHub artifact of the build). SignPath signs
   the program; the workflow bundles the installer around the signed program.
2. `installer`: a zip with `Trimcraft_<version>_x64-setup.exe`. SignPath signs the installer;
   the workflow then creates the updater signature and `latest.json` from it and publishes.

Both configurations take the parameter `version` (the workflow passes the release version, e.g.
`0.1.1`) and only accept files whose product name is `Trimcraft` and product version is that
version, as the SignPath Foundation requires.

Keep the slugs in sync with the repository variables `SIGNPATH_APP_CONFIGURATION_SLUG` and
`SIGNPATH_INSTALLER_CONFIGURATION_SLUG`. If the structure of an artifact changes (another file
name, more files), update the XML here and in SignPath together.
