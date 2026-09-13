import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const clientRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const serious = new Set(['moderate', 'high', 'critical']);

function runAudit(args) {
  const result = spawnSync('npm', ['audit', '--json', ...args], {
    cwd: clientRoot,
    encoding: 'utf8',
    shell: false,
    maxBuffer: 4 * 1024 * 1024,
  });
  if (result.error) throw new Error('npm audit could not execute');
  let report;
  try {
    report = JSON.parse(result.stdout);
  } catch {
    throw new Error('npm audit did not return valid JSON');
  }
  if (report?.auditReportVersion !== 2 || typeof report?.vulnerabilities !== 'object') {
    throw new Error('unsupported npm audit report');
  }
  return { result, report };
}

function counts(report) {
  const values = report.metadata?.vulnerabilities ?? {};
  return {
    moderate: Number(values.moderate ?? 0),
    high: Number(values.high ?? 0),
    critical: Number(values.critical ?? 0),
  };
}

function roots(via) {
  if (!Array.isArray(via)) return [];
  return via.flatMap((item) => {
    if (!item || typeof item !== 'object') return [];
    return [{
      name: String(item.name ?? ''),
      severity: String(item.severity ?? ''),
      range: String(item.range ?? ''),
      url: String(item.url ?? ''),
    }];
  });
}

function summarize(report) {
  return Object.entries(report.vulnerabilities)
    .filter(([, value]) => serious.has(String(value?.severity).toLowerCase()))
    .map(([name, value]) => ({
      package: name,
      severity: String(value.severity),
      direct: Boolean(value.isDirect),
      viaPackages: Array.isArray(value.via) ? value.via.filter((item) => typeof item === 'string').sort() : [],
      advisories: roots(value.via).sort((a, b) => a.url.localeCompare(b.url)),
      nodes: Array.isArray(value.nodes) ? [...value.nodes].sort() : [],
      fixAvailable: value.fixAvailable ?? false,
    }))
    .sort((a, b) => a.package.localeCompare(b.package));
}

function main() {
  const production = runAudit(['--omit=dev']);
  const productionCounts = counts(production.report);
  if (production.result.status !== 0 || Object.values(productionCounts).some((value) => value !== 0)) {
    throw new Error(`production client dependency audit failed: ${JSON.stringify(productionCounts)}`);
  }

  const complete = runAudit([]);
  const output = {
    schemaVersion: 'canonical.client-npm-audit/v1',
    production: { status: 'passed', counts: productionCounts },
    developmentAndBuild: {
      counts: counts(complete.report),
      findings: summarize(complete.report),
    },
  };
  process.stdout.write(`${JSON.stringify(output)}\n`);
}

try {
  main();
} catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exitCode = 1;
}
