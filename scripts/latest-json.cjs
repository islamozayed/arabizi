// Writes latest.json for the updater in the format the Tauri updater (used
// by releases up to 0.9.x) reads, so those installs update to native builds.
// Usage: node scripts/latest-json.cjs <version> <notes-file> <out> <key>=<url>,<sig-file> ...
const { readFileSync, writeFileSync } = require('node:fs');
const [version, notesFile, out, ...entries] = process.argv.slice(2);
const platforms = {};
for (const entry of entries) {
  const [key, rest] = entry.split('=');
  const [url, sigFile] = rest.split(',');
  platforms[key] = { signature: readFileSync(sigFile, 'utf8').trim(), url };
}
const manifest = { version, notes: readFileSync(notesFile, 'utf8').trim(), pub_date: new Date().toISOString(), platforms };
writeFileSync(out, JSON.stringify(manifest, null, 2) + '\n');
