// Sets the app version everywhere it is written down (package.json,
// src-tauri/Cargo.toml, src-tauri/tauri.conf.json). The release tag must be
// v<version>; the updater compares it with the running version.
// usage: node scripts/set-version.mjs 0.2.0
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("usage: node scripts/set-version.mjs <major.minor.patch>");
  process.exit(1);
}
const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/** Replaces the first match of `re` (its group 1 is kept) in a file. */
function replace(file, re, label) {
  const path = join(root, file);
  const text = readFileSync(path, "utf8");
  if (!re.test(text)) throw new Error(`no ${label} in ${file}`);
  writeFileSync(path, text.replace(re, `$1${version}"`));
  console.log(`${file}: ${version}`);
}

replace("package.json", /("version":\s*")[^"]+"/, '"version"');
replace("src-tauri/tauri.conf.json", /("version":\s*")[^"]+"/, '"version"');
replace("src-tauri/Cargo.toml", /(\[package\][^[]*?\nversion\s*=\s*")[^"]+"/, "[package] version");
console.log(`Next: commit, then publish a GitHub release tagged v${version}.`);
