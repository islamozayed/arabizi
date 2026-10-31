const assert = require('node:assert/strict');
const { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { dirname, join, resolve } = require('node:path');
const { spawnSync } = require('node:child_process');
const { test } = require('node:test');

const repo = resolve(__dirname, '..');
for (const ending of ['\n', '\r\n']) {
  test(`release versions and notes work with ${ending === '\n' ? 'LF' : 'CRLF'} checkouts`, () => {
    const fixture = mkdtempSync(join(tmpdir(), 'franconvert-release-'));
    try {
      for (const file of ['scripts/release-notes.cjs', 'app/Cargo.toml', 'Cargo.lock', 'app/tauri.conf.json', 'app/ui/CHANGELOG.json']) {
        const target = join(fixture, file);
        mkdirSync(dirname(target), { recursive: true });
        writeFileSync(target, readFileSync(join(repo, file), 'utf8').replace(/\r?\n/g, ending));
      }
      const version = JSON.parse(readFileSync(join(repo, 'app/tauri.conf.json'), 'utf8')).version;
      const { title } = JSON.parse(readFileSync(join(repo, 'app/ui/CHANGELOG.json'), 'utf8')).versions[0];
      const env = { ...process.env, GITHUB_REF: `refs/tags/v${version}`, GITHUB_OUTPUT: join(fixture, 'output') };
      const run = () => spawnSync(process.execPath, [join(fixture, 'scripts/release-notes.cjs')], { env, encoding: 'utf8' });
      const valid = run();
      assert.equal(valid.status, 0, valid.stderr);
      assert.ok(valid.stdout.startsWith(`${title}\n\n- `), 'notes must open with the latest changelog title');
      assert.doesNotMatch(valid.stdout, /siri|writing tools/i);
      assert.match(readFileSync(env.GITHUB_OUTPUT, 'utf8'), /body<</);
      env.GITHUB_REF = 'refs/tags/v0.0.0';
      assert.notEqual(run().status, 0, 'must reject a mismatched tag');
    } finally {
      rmSync(fixture, { recursive: true, force: true });
    }
  });
}
