#!/usr/bin/env node
// Prints the release notes for a version: an early-access note and the version's section of
// CHANGELOG.md (without its heading). Used by the release workflow; fails when the section is
// missing or empty, so a release cannot go out without notes.
//
//   node scripts/release-notes.mjs 0.1.0

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const EARLY_ACCESS =
  "**Early access.** Trimcraft is young: expect rough edges and back up your configuration " +
  "(Settings, Backup & transfer) before updating. Windows 10/11 only. Install with " +
  "`Trimcraft_<version>_x64-setup.exe`; installed copies update themselves (Settings, Updates). " +
  "Windows code signing (SignPath Foundation): see the " +
  "[code signing policy](https://github.com/trimcraft/trimcraft#code-signing-policy).";

/** The body of `## [version]` in a Keep a Changelog file, or null. */
export function changelogSection(changelog, version) {
  const lines = changelog.split(/\r?\n/);
  const start = lines.findIndex((l) => l.startsWith(`## [${version}]`));
  if (start < 0) return null;
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((l) => l.startsWith("## ") || /^\[[^\]]+\]:\s/.test(l));
  const body = (end < 0 ? rest : rest.slice(0, end)).join("\n").trim();
  return body || null;
}

export function releaseNotes(changelog, version) {
  const section = changelogSection(changelog, version);
  if (!section) return null;
  return `${EARLY_ACCESS.replace("<version>", version)}\n\n${section}\n`;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const version = process.argv[2]?.replace(/^v/, "");
  if (!version) {
    console.error("usage: node scripts/release-notes.mjs <version>");
    process.exit(2);
  }
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const notes = releaseNotes(readFileSync(join(root, "CHANGELOG.md"), "utf8"), version);
  if (!notes) {
    console.error(`release-notes: CHANGELOG.md has no section "## [${version}]" with content`);
    process.exit(1);
  }
  process.stdout.write(notes);
}
