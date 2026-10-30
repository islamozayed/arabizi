const assert = require('node:assert/strict');
const { readFileSync, appendFileSync } = require('node:fs');
const { resolve } = require('node:path');

const root = resolve(__dirname, '..');
const read = path => readFileSync(resolve(root, path), 'utf8').replace(/\r\n/g, '\n');
const version = JSON.parse(read('app/tauri.conf.json')).version;
const cargoVersion = read('app/Cargo.toml').match(/^version = "([^"]+)"/m)?.[1];
const lockedVersion = read('Cargo.lock').match(/name = "franconvert"\nversion = "([^"]+)"/)?.[1];
const notes = JSON.parse(read('app/ui/CHANGELOG.json')).versions[0];
assert.equal(cargoVersion, version, 'Cargo and Tauri versions must match');
assert.equal(lockedVersion, version, 'Cargo.lock version must match');
assert.equal(notes.version, version, 'Latest changelog entry must match');
if (process.env.GITHUB_REF?.startsWith('refs/tags/')) {
  assert.equal(process.env.GITHUB_REF, `refs/tags/v${version}`, 'Release tag must match');
}
const plain = text => text.replace(/<[^>]+>/g, '');
const body = `${notes.title}\n\n${notes.items.map(item => `- ${plain(item)}`).join('\n')}\n`;
if (process.env.GITHUB_OUTPUT) {
  const delimiter = `RELEASE_NOTES_${version.replaceAll('.', '_')}`;
  assert(!body.split('\n').includes(delimiter));
  appendFileSync(process.env.GITHUB_OUTPUT, `body<<${delimiter}\n${body}${delimiter}\n`);
}
process.stdout.write(body);
