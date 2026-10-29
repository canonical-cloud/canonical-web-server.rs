import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

// These narrowly scoped minimums come from RustSec advisories observed by
// canonical-web-server's executed cargo-audit job, not from guessed checksums.
export const rustsecFloors = Object.freeze({
  'ctutils': '0.4.4',
  'quinn-proto': '0.11.18',
  'rustls': '0.23.46',
  'rustls-webpki': '0.103.16',
  'webpki-root-certs': '1.0.10',
  'webpki-roots': '1.0.10',
  'tokio': '1.53.3',
});

function versionParts(version) {
  assert.match(version, /^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/,
    `unreviewed package version ${version}`);
  return version.split('.').map(Number);
}
function compareVersions(a, b) {
  const left = versionParts(a);
  const right = versionParts(b);
  for (let i = 0; i < 3; i += 1) {
    if (left[i] !== right[i]) return Math.sign(left[i] - right[i]);
  }
  return 0;
}

export function rustsecLockFindings(lock) {
  const findings = [];
  assert.match(lock, /^version\s*=\s*\d+\s*$/m, 'Cargo.lock format version required');
  assert.match(lock, /^\[\[package\]\]\s*$/m, 'Cargo.lock packages required');
  for (const entry of lock.split(/^\[\[package\]\]\s*$/m).slice(1)) {
    const name = entry.match(/^name\s*=\s*"([^"]+)"\s*$/m)?.[1];
    if (!name || !(name in rustsecFloors)) continue;
    const source = entry.match(/^source\s*=\s*"([^"]+)"\s*$/m)?.[1];
    if (!source?.startsWith('registry+')) continue;
    const version = entry.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
    assert.ok(version, `${name}: missing lockfile version`);
    const minimum = rustsecFloors[name];
    if (compareVersions(version, minimum) < 0 || (name === 'tokio' && version === '1.54.0')) {
      findings.push({ name, version, minimum, reason: name === 'tokio'
        ? 'RUSTSEC-2026-0386 / RUSTSEC-2026-0384 (1.54.0 is vulnerable)'
        : 'RustSec advisory minimum not satisfied' });
    }
  }
  return findings;
}

if (process.argv[1] && import.meta.url === new URL('file://' + process.argv[1]).href) {
  const lock = await readFile(new URL('../Cargo.lock', import.meta.url), 'utf8');
  const findings = rustsecLockFindings(lock);
  for (const item of findings) {
    console.error(`${item.name} ${item.version} requires >=${item.minimum}: ${item.reason}`);
  }
  if (findings.length) {
    console.error(`${findings.length} locked package(s) still need verified RustSec remediation; cargo audit and full trusted Cargo build also required.`);
    process.exitCode = 1;
  } else {
    console.log('All seven scoped RustSec package floors satisfied where present; full cargo audit still required.');
  }
}
