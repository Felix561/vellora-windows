#!/usr/bin/env node
// Inspect the unpacked executable, not just a compressed installer container.
// Report counts only: matching paths can contain a developer's private details.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';

const pathPatterns = {
  windowsUserProfile: /[a-z]:[\\/]+Users[\\/]+[^\\/\x00-\x1f<>:"|?*]+/gi,
  unixUserProfile: /\/(?:Users|home)\/[^/\x00-\x1f<>:"|?*]+\//gi,
  windowsSourceOrDebugPath:
    /(?<![a-z0-9])[a-z]:(?:\\|\/(?!\/))(?:[^\x00-\x1f<>:"|?*]+?[\\/])+[^\x00-\x1f<>:"|?*]+?\.(?:rs|pdb|c|cpp|h|hpp|lib|obj|ts|tsx|js|json|html)(?=[^a-z0-9_]|$)/gi,
  networkSourceOrDebugPath:
    /\\\\(?:[?.]\\UNC\\)?(?![?.]\\)[^\\/\x00-\x1f<>:"|?*]+\\[^\x00-\x1f<>:"|?*]+?\.(?:rs|pdb|c|cpp|h|hpp|lib|obj|ts|tsx|js|json|html)(?=[^a-z0-9_]|$)/gi,
};

function escaped(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function forbiddenPathPattern(value) {
  const parts = value.replaceAll('\\', '/').replace(/\/+$/, '').split('/');
  if (!value.trim() || parts.filter(Boolean).length < 2) {
    throw new Error('A forbidden path must contain at least two nonempty components.');
  }
  return new RegExp(parts.map(escaped).join('[\\\\/]'), 'gi');
}

export function inspectExecutable(data, forbiddenPaths = []) {
  const texts = [
    data.toString('utf8'),
    data.toString('utf16le'),
    data.subarray(1).toString('utf16le'),
  ];
  const counts = {};
  for (const [category, pattern] of Object.entries(pathPatterns)) {
    counts[category] = texts.reduce((total, text) => total + [...text.matchAll(pattern)].length, 0);
  }
  counts.explicitForbiddenBuildRoot = forbiddenPaths.reduce((total, value) => {
    const pattern = forbiddenPathPattern(value);
    return total + texts.reduce((sum, text) => sum + [...text.matchAll(pattern)].length, 0);
  }, 0);
  return {
    bytes: data.length,
    sha256: createHash('sha256').update(data).digest('hex'),
    findingCounts: counts,
    passed: Object.values(counts).every((count) => count === 0),
  };
}

function selfTest() {
  const profile = 'C:\\Users\\Synthetic User\\.cargo\\registry\\crate\\src\\lib.rs';
  const ascii = inspectExecutable(Buffer.from(`prefix\0${profile}\0`, 'utf8'));
  assert.equal(ascii.findingCounts.windowsUserProfile, 1);
  assert.equal(ascii.findingCounts.windowsSourceOrDebugPath, 1);
  assert.equal(ascii.passed, false);

  const utf16 = inspectExecutable(Buffer.from(profile, 'utf16le'));
  assert.equal(utf16.findingCounts.windowsUserProfile, 1);
  const oddUtf16 = inspectExecutable(Buffer.concat([Buffer.from([0xff]), Buffer.from(profile, 'utf16le')]));
  assert.equal(oddUtf16.findingCounts.windowsUserProfile, 1);

  const source = inspectExecutable(Buffer.from('C:/private workspace/project/src/main.rs\0'));
  assert.equal(source.findingCounts.windowsSourceOrDebugPath, 1);
  assert.equal(source.passed, false);
  assert.equal(inspectExecutable(Buffer.from('/home/synthetic-user/project/main.rs')).passed, false);
  assert.equal(inspectExecutable(Buffer.from('\\\\synthetic-host\\private-share\\project\\symbols.pdb')).passed, false);
  assert.equal(inspectExecutable(Buffer.from('\\\\?\\UNC\\synthetic-host\\private-share\\project\\symbols.pdb')).passed, false);
  assert.equal(inspectExecutable(Buffer.from('\\\\.\\UNC\\synthetic-host\\private-share\\project\\symbols.pdb')).passed, false);
  // Windows' public NUL device literal can sit directly before a neutral Rust
  // source-location string in the executable's concatenated string section.
  assert.equal(inspectExecutable(Buffer.from('\\\\.\\NUL/rustc/anonymous/library\\std\\src\\io.rs')).passed, true);
  // A repeated-character lookup table can end with doubled backslash bytes
  // directly beside a remapped dependency source path. A slash cannot be part
  // of the UNC server component, so this is not an absolute network path.
  assert.equal(inspectExecutable(Buffer.from('[[[[\\\\\\\\]]]]^^^^____aaaa/cargo\\registry\\synthetic-crate\\src\\lib.rs')).passed, true);

  const explicitlyForbidden = inspectExecutable(Buffer.from('D:\\build area\\private project\\assets\\icon.png'), ['D:/build area/private project']);
  assert.equal(explicitlyForbidden.findingCounts.explicitForbiddenBuildRoot, 1);
  const unicodeForbidden = inspectExecutable(Buffer.from('D:\\synthetic üser\\project\\assets\\icon.png'), ['D:/synthetic üser/project']);
  assert.equal(unicodeForbidden.findingCounts.explicitForbiddenBuildRoot, 1);
  assert.throws(() => inspectExecutable(Buffer.from('safe'), ['C:']));

  const remapped = inspectExecutable(Buffer.from([
    '/vellora/src/main.rs', '/cargo/registry/example/src/lib.rs',
    '/rust/library/std/src/path.rs', '%APPDATA%/Vellora/settings.json',
    'https://example.invalid/privacy.json', 'ms-settings:privacy-microphone',
  ].join('\0')));
  assert.equal(remapped.passed, true);
  process.stdout.write('Release privacy scanner self-test passed (synthetic data only).\n');
}

function main(args) {
  if (args.length === 1 && args[0] === '--self-test') {
    selfTest();
    return;
  }
  let file;
  const forbiddenPaths = [];
  for (let index = 0; index < args.length; index += 1) {
    const option = args[index];
    const value = args[index + 1];
    if ((option !== '--file' && option !== '--forbid-path') || !value || value.startsWith('--')) {
      throw new Error('Usage: node check-release-privacy.mjs --file <unpacked-exe> [--forbid-path <build-root>] (repeatable), or --self-test.');
    }
    if (option === '--file') {
      if (file) throw new Error('Supply exactly one executable file.');
      file = value;
    } else {
      forbiddenPaths.push(value);
    }
    index += 1;
  }
  if (!file) throw new Error('Supply the unpacked release executable with --file.');
  let data;
  try {
    data = readFileSync(file);
  } catch {
    throw new Error('Unable to read the executable. Check the supplied file locally; its path is omitted from this report.');
  }
  if (data.length < 2 || data[0] !== 0x4d || data[1] !== 0x5a) {
    throw new Error('The input is not a Windows executable. Scan the unpacked release EXE.');
  }
  const result = inspectExecutable(data, forbiddenPaths);
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  if (!result.passed) process.exitCode = 1;
}

try {
  main(process.argv.slice(2));
} catch (error) {
  // Only our static messages reach stderr; OS errors could include personal paths.
  process.stderr.write(`${error.message}\n`);
  process.exitCode = 2;
}
