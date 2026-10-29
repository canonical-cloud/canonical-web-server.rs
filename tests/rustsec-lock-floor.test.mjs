import assert from 'node:assert/strict';
import { test } from 'node:test';
import { rustsecLockFindings, rustsecFloors } from '../scripts/rustsec-lock-floor.mjs';
const wrap = (...items) => 'version = 4\n\n' + items.map(([name, version, source = 'registry+https://github.com/rust-lang/crates.io-index']) => `[[package]]\nname = "${name}"\nversion = "${version}"\nsource = "${source}"\n`).join('\n');
test('detect every known vulnerable minimum in the real lock snapshot', () => {
  const before = [['ctutils','0.4.2'],['quinn-proto','0.11.16'],['rustls','0.23.45'],['rustls-webpki','0.103.15'],['webpki-root-certs','1.0.9'],['webpki-roots','1.0.9'],['tokio','1.53.1']];
  assert.deepEqual(rustsecLockFindings(wrap(...before)).map(x => x.name).sort(), before.map(x=>x[0]).sort());
});
test('accept reviewed minimums and newer patched versions', () => {
  assert.deepEqual(rustsecLockFindings(wrap(...Object.entries(rustsecFloors))), []);
  assert.deepEqual(rustsecLockFindings(wrap(['tokio','1.54.1'],['quinn-proto','0.11.19'])), []);
});
test('reject vulnerable Tokio 1.54.0 even though above 1.53.3', () => {
  assert.deepEqual(rustsecLockFindings(wrap(['tokio', '1.54.0'])).map(x => x.name), ['tokio']);
});
test('do not treat unrelated packages and git sources as crates.io advisories', () => {
  assert.deepEqual(rustsecLockFindings(wrap(['unrelated','0.1.0'],['tokio','0.1.0','git+https://github.com/example/repo'])), []);
});
test('reject malformed lockfiles and ambiguous versions', () => {
  assert.throws(() => rustsecLockFindings('not a lockfile'), /Cargo.lock/);
  assert.throws(() => rustsecLockFindings(wrap(['tokio','1.54.0-rc1'])), /unreviewed package version/);
});
