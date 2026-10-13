#!/usr/bin/env node
// Checks that the version is the same everywhere it is written down: Cargo.toml
// ([workspace.package]), package.json, package-lock.json and src-tauri/tauri.conf.json.
// With --tag vX.Y.Z (the release workflow) it must also match the tag.
//
//   node scripts/check-versions.mjs [--tag v0.1.0]
//
// Prints the version; exits with 1 and the differences otherwise. The same check runs as a
// Rust test (src-tauri/src/updates.rs), so CI fails on a mismatch.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path) => readFileSync(join(root, path), "utf8");

/** The version in Cargo.toml's [workspace.package] section. */
export function cargoVersion(toml) {
  const section = toml.split(/^\[/m).find((s) => s.startsWith("workspace.package]"));
  const match = section?.match(/^version\s*=\s*"([^"]+)"/m);
  return match?.[1] ?? null;
}

/** All versions by file; problems as text (empty when all agree, and with the tag if given). */
export function checkVersions(files, tag) {
  const versions = {
    "Cargo.toml": cargoVersion(files.cargo),
    "package.json": JSON.parse(files.packageJson).version ?? null,
    "package-lock.json": JSON.parse(files.packageLock).version ?? null,
    "src-tauri/tauri.conf.json": JSON.parse(files.tauriConf).version ?? null,
  };
  const problems = [];
  const values = new Set(Object.values(versions));
  if (values.size !== 1 || values.has(null)) {
    problems.push(
      "versions differ: " +
        Object.entries(versions)
          .map(([f, v]) => `${f} ${v ?? "(none)"}`)
          .join(", "),
    );
  }
  const version = versions["Cargo.toml"];
  if (tag !== undefined && tag !== `v${version}`) {
    problems.push(`tag ${tag} does not match version ${version} (expected v${version})`);
  }
  return { version, problems };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const i = process.argv.indexOf("--tag");
  const tag = i > 0 ? process.argv[i + 1] : undefined;
  const { version, problems } = checkVersions(
    {
      cargo: read("Cargo.toml"),
      packageJson: read("package.json"),
      packageLock: read("package-lock.json"),
      tauriConf: read("src-tauri/tauri.conf.json"),
    },
    tag,
  );
  if (problems.length) {
    for (const p of problems) console.error(`check-versions: ${p}`);
    process.exit(1);
  }
  console.log(version);
}
