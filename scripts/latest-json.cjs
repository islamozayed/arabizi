// Writes latest.json for the updater in the format the Tauri updater (used
// by releases up to 0.9.x) reads, so those installs update to native builds.
// Usage: node scripts/latest-json.cjs <version> <notes-file> <out> <key>=<url>,<sig-file> ...
// With LATEST_BASE=<existing latest.json>, keeps that file's other platforms
// (adding a platform to a release that's already out).
const { readFileSync, writeFileSync } = require('node:fs');
const [version, notesFile, out, ...entries] = process.argv.slice(2);
const base = process.env.LATEST_BASE ? JSON.parse(readFileSync(process.env.LATEST_BASE, 'utf8')) : null;
if (base && base.version !== version) {
  throw new Error(`existing latest.json is for ${base.version}, not ${version}`);
}
const platforms = { ...(base?.platforms ?? {}) };
for (const entry of entries) {
  const [key, rest] = entry.split('=');
  const [url, sigFile] = rest.split(',');
  platforms[key] = { signature: readFileSync(sigFile, 'utf8').trim(), url };
}
const manifest = {
  version,
  notes: readFileSync(notesFile, 'utf8').trim(),
  pub_date: base?.pub_date ?? new Date().toISOString(),
  platforms,
};
writeFileSync(out, JSON.stringify(manifest, null, 2) + '\n');
