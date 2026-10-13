// Appends what cargo-about does not see to THIRD_PARTY_LICENSES: the C libraries built into
// trimcraft.exe from source (SDL3 and the hidapi copy inside it) and the npm packages bundled
// into the UI. Run by scripts/third-party-licenses.sh with the output file as argument.
import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";

const out = process.argv[2];
const RULE = "=".repeat(80);
const section = (title, users, text) =>
  appendFileSync(out, `\n${RULE}\n${title}\n\nUsed by:\n${users.map((u) => `  - ${u}`).join("\n")}\n\n${text.trim()}\n`);

// --- C libraries: SDL3, built from the sdl3-src crate and linked statically ------------------
const metadata = JSON.parse(
  execFileSync("cargo", ["metadata", "--format-version", "1", "--locked"], { maxBuffer: 64 << 20 }).toString(),
);
const sdl = metadata.packages.find((p) => p.name === "sdl3-src");
if (!sdl) throw new Error("sdl3-src not found in cargo metadata");
const sdlDir = join(dirname(sdl.manifest_path), "SDL");
section(
  "SDL 3 (Zlib)",
  [`SDL ${sdl.version} (https://libsdl.org), built from source by sdl3-src ${sdl.version}`],
  readFileSync(join(sdlDir, "LICENSE.txt"), "utf8"),
);
section(
  "HIDAPI, as included in SDL (BSD-3-Clause, the option chosen from its licences)",
  [`hidapi inside SDL ${sdl.version} (https://github.com/libusb/hidapi)`],
  readFileSync(join(sdlDir, "src", "hidapi", "LICENSE-bsd.txt"), "utf8"),
);

// --- npm packages bundled into the UI (runtime code only; build tools are not shipped) ------
const NPM = ["@tauri-apps/api", "svelte", "esm-env", "clsx", "devalue"];
for (const name of NPM) {
  const dir = join("node_modules", name);
  const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
  const file = ["LICENSE", "LICENSE.md", "LICENSE.txt", "license", "LICENSE-MIT"].find((f) => {
    try {
      readFileSync(join(dir, f));
      return true;
    } catch {
      return false;
    }
  });
  if (!file) throw new Error(`no licence file in ${dir}`);
  section(
    `${name} (${pkg.license})`,
    [`${name} ${pkg.version} (npm)`],
    readFileSync(join(dir, file), "utf8"),
  );
}
