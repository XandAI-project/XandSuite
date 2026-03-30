#!/usr/bin/env node
/**
 * collect-artifacts.js
 *
 * Scans src-tauri/target/release/bundle/ for the platform-specific build
 * outputs produced by `npm run tauri build` and copies them into:
 *
 *   version/executables/<version>/
 *
 * Supported outputs:
 *   Windows  →  nsis/*.exe
 *   macOS    →  dmg/*.dmg
 *   Linux    →  appimage/*.AppImage
 *
 * Usage:
 *   node scripts/collect-artifacts.js
 */

import { readFileSync, mkdirSync, copyFileSync, readdirSync, existsSync } from "fs";
import { join, basename, dirname, resolve } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, "..");

// Read version from package.json
const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
const version = pkg.version;

const BUNDLE_DIR = join(ROOT, "src-tauri", "target", "release", "bundle");
const OUT_DIR = join(ROOT, "version", "executables", version);

// (subdir, file extension glob) pairs to scan
const TARGETS = [
  { dir: "nsis",     ext: ".exe"      },
  { dir: "dmg",      ext: ".dmg"      },
  { dir: "appimage", ext: ".AppImage" },
];

mkdirSync(OUT_DIR, { recursive: true });

let copied = 0;

for (const { dir, ext } of TARGETS) {
  const searchDir = join(BUNDLE_DIR, dir);
  if (!existsSync(searchDir)) {
    console.log(`  skip  ${dir}/  (not built on this platform)`);
    continue;
  }

  const files = readdirSync(searchDir, { recursive: true })
    .filter((f) => typeof f === "string" && f.endsWith(ext));

  if (files.length === 0) {
    console.log(`  skip  ${dir}/  (no ${ext} files found)`);
    continue;
  }

  for (const rel of files) {
    const src  = join(searchDir, rel);
    const dest = join(OUT_DIR, basename(rel));
    copyFileSync(src, dest);
    console.log(`  copy  ${basename(rel)}  →  version/executables/${version}/`);
    copied++;
  }
}

if (copied === 0) {
  console.warn(
    "\nNo artifacts found. Run `npm run build:dist` first on each target platform.\n"
  );
  process.exit(1);
}

console.log(`\nDone — ${copied} artifact(s) collected into version/executables/${version}/\n`);
