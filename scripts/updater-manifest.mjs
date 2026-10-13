#!/usr/bin/env node
// Prints the updater manifest (latest.json) for a release whose installer was signed after the
// build: the release workflow's SignPath path, where tauri-action cannot write it. Same shape as
// tauri-action's file with updaterJsonPreferNsis: the NSIS installer under `windows-x86_64` and
// `windows-x86_64-nsis`, the updater signature as the content of its .sig file.
//
//   node scripts/updater-manifest.mjs <version> <installer.exe> <notes.md> <download base url>

import { readFileSync } from "node:fs";
import { basename } from "node:path";
import { fileURLToPath } from "node:url";

/** The manifest object; `signature` is the .sig file's text, `url` the installer's download URL. */
export function updaterManifest({ version, notes, signature, url, pubDate }) {
  const entry = { signature: signature.trim(), url };
  return {
    version,
    notes,
    pub_date: pubDate,
    platforms: {
      "windows-x86_64": entry,
      "windows-x86_64-nsis": entry,
    },
  };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const [version, installer, notesFile, baseUrl] = process.argv.slice(2);
  if (!version || !installer || !notesFile || !baseUrl) {
    console.error(
      "usage: node scripts/updater-manifest.mjs <version> <installer.exe> <notes.md> <download base url>",
    );
    process.exit(2);
  }
  const manifest = updaterManifest({
    version: version.replace(/^v/, ""),
    notes: readFileSync(notesFile, "utf8"),
    signature: readFileSync(`${installer}.sig`, "utf8"),
    url: `${baseUrl.replace(/\/$/, "")}/${encodeURIComponent(basename(installer))}`,
    pubDate: new Date().toISOString(),
  });
  process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
}
