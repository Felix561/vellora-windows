#!/usr/bin/env node
// Prepare exact, unmodified covered sources beside a PRIVATE review artifact.
// This neither publishes an installer nor establishes a public download service.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = fileURLToPath(new URL('../..', import.meta.url));
const reviewedMpl = [
  'cssparser@0.36.0', 'cssparser-macros@0.6.1', 'dtoa-short@0.3.5',
  'option-ext@0.2.0', 'selectors@0.36.1',
];
const nsis = {
  version: '3.11',
  commit: '7359413009afd4f0fff472d841fc2f2cc0e0a5f8',
  url: 'https://api.github.com/repos/nsis-dev/nsis/zipball/7359413009afd4f0fff472d841fc2f2cc0e0a5f8',
  file: 'nsis-3.11-source.zip',
  sha256: '48c2b3b70e44b3df355e580e83c4107ff1e5b9ba4d7966bc9950aff0ab7d72ac',
};
// Verified against upstream annotated tag v311 on 2026-10-04. This archive's
// COPYING matched the exact commit and it contains Source/7zip (LZMA) and
// Source/exehead. Hash pinning fails closed if GitHub changes archive encoding.
const safeMessages = new Set();
function fail(message) {
  safeMessages.add(message);
  throw new Error(message);
}
function digest(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}
function checkedArchive(bytes, expected, component) {
  if (!/^[a-f0-9]{64}$/.test(expected) || digest(bytes) !== expected) {
    fail(`Source archive checksum mismatch for ${component}.`);
  }
  return bytes;
}
function lockPackage(lock, name, version) {
  const matches = lock.split(/^\[\[package\]\]\s*$/m).filter((block) =>
    new RegExp(`^name = "${name}"$`, 'm').test(block) &&
    new RegExp(`^version = "${version.replaceAll('.', '\\.')}"$`, 'm').test(block));
  if (matches.length !== 1) fail('Expected exactly one reviewed component in Cargo.lock.');
  const source = matches[0].match(/^source = "([^"]+)"$/m)?.[1];
  const checksum = matches[0].match(/^checksum = "([a-f0-9]{64})"$/m)?.[1];
  if (source !== 'registry+https://github.com/rust-lang/crates.io-index' || !checksum) {
    fail('Reviewed MPL component must have a crates.io checksum in Cargo.lock.');
  }
  return checksum;
}
function reviewedPackages(metadata) {
  const resolved = new Set(metadata.resolve.nodes.map((node) => node.id));
  const packages = metadata.packages.filter((crate) => resolved.has(crate.id) && crate.license?.includes('MPL'));
  const identities = packages.map((crate) => `${crate.name}@${crate.version}`).sort();
  if (JSON.stringify(identities) !== JSON.stringify([...reviewedMpl].sort()) ||
      packages.some((crate) => crate.license !== 'MPL-2.0')) {
    fail('Windows MPL dependency inventory changed; update the source/licensing review before release.');
  }
  return packages.sort((a, b) => a.name.localeCompare(b.name));
}
function selfTest() {
  const checksum = digest(Buffer.from('synthetic archive'));
  const fixture = `version = 4\n\n[[package]]\nname = "option-ext"\nversion = "0.2.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "${checksum}"\n`;
  assert.equal(lockPackage(fixture, 'option-ext', '0.2.0'), checksum);
  assert.throws(() => lockPackage(fixture + fixture, 'option-ext', '0.2.0'));
  assert.throws(() => lockPackage(fixture.replace('checksum =', 'missing ='), 'option-ext', '0.2.0'));
  assert.throws(() => checkedArchive(Buffer.from('changed bytes'), checksum, 'synthetic component'));
  assert.deepEqual(checkedArchive(Buffer.from('synthetic archive'), checksum, 'synthetic component'), Buffer.from('synthetic archive'));
  const packages = reviewedMpl.map((entry) => {
    const [name, version] = entry.split('@');
    return { name, version, id: entry, license: 'MPL-2.0' };
  });
  const metadata = { packages, resolve: { nodes: packages.map(({ id }) => ({ id })) } };
  assert.equal(reviewedPackages(metadata).length, 5);
  assert.throws(() => reviewedPackages({ ...metadata, packages: [...packages, { id: 'extra', license: 'MPL-2.0' }], resolve: { nodes: [...metadata.resolve.nodes, { id: 'extra' }] } }));
  process.stdout.write('Release source companion self-test passed (synthetic data only).\n');
}
async function main(args) {
  if (args.length === 1 && args[0] === '--self-test') return selfTest();
  if (args.length && (args.length !== 2 || args[0] !== '--output-dir')) {
    fail('Usage: node prepare-release-sources.mjs [--output-dir <new-directory>] or --self-test.');
  }
  const output = args.length ? path.resolve(args[1]) :
    path.join(process.env.RUNNER_TEMP || os.tmpdir(), `vellora-release-sources-${randomUUID()}`);
  if (fs.existsSync(output)) fail('Source companion output directory must not already exist.');
  const nativeNotices = fs.readFileSync(path.join(projectRoot, 'THIRD_PARTY_NOTICES_NATIVE.md'), 'utf8');
  const licensingReview = fs.readFileSync(path.join(projectRoot, 'docs/LICENSING_REVIEW.md'), 'utf8');
  if (!nativeNotices.includes(`NSIS ${nsis.version}`) || !nativeNotices.includes(nsis.commit) ||
      !licensingReview.includes('MPL-2.0') || !licensingReview.includes(nsis.commit)) {
    fail('Native/licensing review must identify the reviewed NSIS and MPL sources.');
  }
  let metadata;
  try {
    metadata = JSON.parse(execFileSync('cargo', [
      'metadata', '--locked', '--format-version', '1',
      '--filter-platform', 'x86_64-pc-windows-msvc', '--manifest-path', 'src-tauri/Cargo.toml',
    ], { cwd: projectRoot, encoding: 'utf8', maxBuffer: 20 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] }));
  } catch {
    fail('Unable to resolve the locked Windows source inventory; Cargo output is omitted.');
  }
  const packages = reviewedPackages(metadata);
  const lock = fs.readFileSync(path.join(projectRoot, 'src-tauri/Cargo.lock'), 'utf8');
  fs.mkdirSync(output, { recursive: true });
  const components = [];
  for (const crate of packages) {
    const checksum = lockPackage(lock, crate.name, crate.version);
    const filename = `${crate.name}-${crate.version}.crate`;
    // Cargo metadata identifies the exact registry index that supplied this
    // crate. Use its immutable published archive, not a possibly edited src dir.
    const sourceDirectory = path.dirname(crate.manifest_path);
    const registryIndex = path.basename(path.dirname(sourceDirectory));
    const registryRoot = path.resolve(sourceDirectory, '../../..');
    const cachedArchive = path.join(registryRoot, 'cache', registryIndex, filename);
    const bytes = checkedArchive(fs.readFileSync(cachedArchive), checksum, `${crate.name} ${crate.version}`);
    fs.writeFileSync(path.join(output, filename), bytes, { flag: 'wx' });
    components.push({
      component: crate.name, version: crate.version, license: crate.license,
      file: filename, sha256: checksum,
      archiveOrigin: `https://static.crates.io/crates/${crate.name}/${filename}`,
      upstreamSource: `https://docs.rs/crate/${crate.name}/${crate.version}/source/`,
      changes: 'Unmodified published crate archive; checksum matches committed Cargo.lock',
    });
  }
  let downloaded;
  try {
    const response = await fetch(nsis.url, {
      headers: { 'User-Agent': 'Vellora-release-source-review', Accept: 'application/vnd.github+json' },
      signal: AbortSignal.timeout(90_000),
    });
    if (!response.ok) fail('Official NSIS source archive download failed.');
    downloaded = Buffer.from(await response.arrayBuffer());
  } catch {
    fail('Official NSIS source archive download failed; network details are omitted.');
  }
  checkedArchive(downloaded, nsis.sha256, `NSIS ${nsis.version}`);
  fs.writeFileSync(path.join(output, nsis.file), downloaded, { flag: 'wx' });
  components.push({
    component: 'NSIS installer/LZMA source', version: nsis.version,
    license: 'NSIS license catalog: zlib and CPL-1.0 for LZMA with linking exception',
    file: nsis.file, sha256: nsis.sha256, commit: nsis.commit, archiveOrigin: nsis.url,
    upstreamSource: `https://github.com/nsis-dev/nsis/tree/${nsis.commit}`,
    changes: 'Unmodified official source archive, includes LZMA and installer/uninstaller stub sources',
  });
  for (const filename of ['LICENSE', 'THIRD_PARTY_NOTICES.md', 'THIRD_PARTY_NOTICES_NATIVE.md']) {
    fs.copyFileSync(path.join(projectRoot, filename), path.join(output, filename), fs.constants.COPYFILE_EXCL);
  }
  const recovered = JSON.parse(fs.readFileSync(path.join(projectRoot, '.github/licenses/verified-upstream-notices.json'), 'utf8'))
    .filter((record) => packages.some((crate) => crate.name === record.name && crate.version === record.version));
  for (const record of recovered) {
    for (const notice of record.notices) checkedArchive(Buffer.from(notice.text), notice.sha256, `${record.name} recovered notice`);
  }
  fs.writeFileSync(path.join(output, 'RECOVERED_MPL_NOTICES.json'), `${JSON.stringify(recovered, null, 2)}\n`, { flag: 'wx' });
  const version = JSON.parse(fs.readFileSync(path.join(projectRoot, 'package.json'), 'utf8')).version;
  const manifest = {
    schemaVersion: 1, application: 'Vellora', applicationVersion: version,
    sourceCommit: /^[a-f0-9]{40}$/i.test(process.env.GITHUB_SHA || '') ? process.env.GITHUB_SHA : null,
    purpose: 'Exact-source companion for review; publish together with an eventual approved binary release',
    scope: 'Conservative covered MPL Windows graph and NSIS LZMA/stubs; no SDK, compiler, runtime or tool binaries',
    components,
  };
  fs.writeFileSync(path.join(output, 'SOURCE_MANIFEST.json'), `${JSON.stringify(manifest, null, 2)}\n`, { flag: 'wx' });
  fs.writeFileSync(path.join(output, 'README.txt'), [
    `Vellora ${version} exact-source companion`, '',
    'The .crate files are gzip/tar source archives; the NSIS file is a ZIP source archive.',
    'Each source archive is unchanged and has a SHA256 in SOURCE_MANIFEST.json.',
    'Read original headers/licenses inside the archives and the accompanying notice files.',
    'RECOVERED_MPL_NOTICES.json preserves verified license texts omitted by some crate packages.',
    'A listed build-time component is not proof that its machine code is distributed.',
    'This companion adds no obligation to relicense independent Vellora files under MPL or CPL.',
    'Private review artifacts expire. A public binary release must offer a durable, reliable',
    'download location for this companion and tell recipients where to obtain covered source.',
    'Neither this script nor this artifact authorizes publication.', '',
  ].join('\n'), { flag: 'wx' });
  const checksumLines = fs.readdirSync(output).sort().map((filename) =>
    `${digest(fs.readFileSync(path.join(output, filename)))}  ${filename}`);
  fs.writeFileSync(path.join(output, 'SHA256SUMS.txt'), `${checksumLines.join('\n')}\n`, { flag: 'wx' });
  if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, `source-directory=${output}\n`);
  process.stdout.write('Prepared exact-source companion: five locked MPL crate archives and NSIS 3.11 source.\n');
}
try {
  await main(process.argv.slice(2));
} catch (error) {
  // Raw OS/Cargo/network errors can contain a contributor's private directories.
  process.stderr.write(`${safeMessages.has(error.message) ? error.message : 'Source companion preparation failed; local paths and tool output are omitted.'}\n`);
  process.exitCode = 1;
}
