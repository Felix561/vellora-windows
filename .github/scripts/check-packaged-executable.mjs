#!/usr/bin/env node
// Tauri temporarily patches bundle-type bytes for NSIS, then restores its main
// release EXE. Verify the exact reviewed transformation rather than pretending
// that the restored EXE is byte-identical to the installed application.
// Reviewed CLI 2.11.2 implementation at its immutable tag commit:
// https://github.com/tauri-apps/tauri/blob/499df79be65ef8c0670abc0207cd9e37b55d8491/crates/tauri-bundler/src/bundle.rs
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = fileURLToPath(new URL('../..', import.meta.url));
const reviewedCliVersion = '2.11.2';
const originalMarker = Buffer.from('__TAURI_BUNDLE_TYPE_VAR_UNK', 'ascii');
const packagedMarker = Buffer.from('__TAURI_BUNDLE_TYPE_VAR_NSS', 'ascii');
const safeMessages = new Set();
function fail(message, diagnostic) {
  safeMessages.add(message);
  const error = new Error(message);
  if (diagnostic) error.safeDiagnostic = diagnostic;
  throw error;
}
function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

export function verifyPackagedBytes(compiled, payload, cliVersion) {
  if (cliVersion !== reviewedCliVersion) {
    fail('Locked Tauri CLI version changed; review its binary-patching implementation before release.');
  }
  if (compiled.length < 2 || compiled[0] !== 0x4d || compiled[1] !== 0x5a ||
      payload.length < 2 || payload[0] !== 0x4d || payload[1] !== 0x5a) {
    fail('Compiled and packaged inputs must both be Windows executable bytes.');
  }
  const markerOffset = compiled.indexOf(originalMarker);
  if (markerOffset < 0 || compiled.indexOf(originalMarker, markerOffset + originalMarker.length) >= 0) {
    fail('Expected exactly one original Tauri bundle-type marker in the compiled executable.');
  }
  const expected = Buffer.from(compiled);
  packagedMarker.copy(expected, markerOffset);
  const originalHash = sha256(compiled);
  const expectedHash = sha256(expected);
  const payloadHash = sha256(payload);
  if (payload.length !== expected.length || payloadHash !== expectedHash) {
    // No normalization, ignored-byte regions, checksum exceptions, or arbitrary
    // token replacements: every payload byte must equal the reviewed result.
    const commonBytes = Math.min(expected.length, payload.length);
    let firstDifferentByteOffset = 0;
    while (firstDifferentByteOffset < commonBytes && expected[firstDifferentByteOffset] === payload[firstDifferentByteOffset]) {
      firstDifferentByteOffset += 1;
    }
    fail('Installer executable differs from the exact reviewed Tauri NSIS patch of the compiled build.', {
      executableMatchesReviewedNsisPatch: false,
      compiledExecutableSha256: originalHash,
      expectedPatchedExecutableSha256: expectedHash,
      payloadExecutableSha256: payloadHash,
      compiledExecutableBytes: compiled.length,
      payloadExecutableBytes: payload.length,
      firstDifferentByteOffset,
    });
  }
  return {
    compiledExecutableSha256: originalHash,
    expectedPatchedExecutableSha256: expectedHash,
    payloadExecutableSha256: payloadHash,
    compiledExecutableBytes: compiled.length,
    payloadExecutableBytes: payload.length,
    executableMatchesReviewedNsisPatch: true,
    bundleTypePatch: {
      reviewedTauriCliVersion: reviewedCliVersion,
      originalMarker: originalMarker.toString('ascii'),
      packagedMarker: packagedMarker.toString('ascii'),
      offset: markerOffset,
      markerCount: 1,
    },
  };
}

function selfTest() {
  const compiled = Buffer.concat([Buffer.from('MZ\0synthetic-header\0'), originalMarker, Buffer.from('\0synthetic-body')]);
  const patched = Buffer.from(compiled);
  packagedMarker.copy(patched, compiled.indexOf(originalMarker));
  const valid = verifyPackagedBytes(compiled, patched, reviewedCliVersion);
  assert.equal(valid.executableMatchesReviewedNsisPatch, true);
  assert.equal(valid.expectedPatchedExecutableSha256, valid.payloadExecutableSha256);
  assert.notEqual(valid.compiledExecutableSha256, valid.payloadExecutableSha256);
  assert.throws(() => verifyPackagedBytes(compiled, compiled, reviewedCliVersion));
  assert.throws(() => verifyPackagedBytes(compiled, patched, '0.0.0'));
  assert.throws(() => verifyPackagedBytes(Buffer.from('MZ\0missing-marker'), patched, reviewedCliVersion));
  assert.throws(() => verifyPackagedBytes(Buffer.concat([compiled, originalMarker]), patched, reviewedCliVersion));
  const outsideChange = Buffer.from(patched);
  outsideChange[outsideChange.length - 1] ^= 1;
  assert.throws(() => verifyPackagedBytes(compiled, outsideChange, reviewedCliVersion));
  const wrongMarker = Buffer.from(patched);
  wrongMarker[compiled.indexOf(originalMarker) + originalMarker.length - 1] = 0x49;
  assert.throws(() => verifyPackagedBytes(compiled, wrongMarker, reviewedCliVersion));
  assert.throws(() => verifyPackagedBytes(compiled, patched.subarray(0, patched.length - 1), reviewedCliVersion));
  assert.throws(() => verifyPackagedBytes(compiled, Buffer.concat([patched, originalMarker]), reviewedCliVersion));
  const notExecutable = Buffer.from(patched);
  notExecutable[0] = 0;
  assert.throws(() => verifyPackagedBytes(compiled, notExecutable, reviewedCliVersion));
  assert.equal(compiled.includes(originalMarker), true);
  process.stdout.write('Packaged executable self-test passed: exact NSIS marker patch required; all unrelated changes rejected.\n');
}

function main(args) {
  if (args.length === 1 && args[0] === '--self-test') return selfTest();
  let compiledFile;
  let payloadFile;
  const forbiddenPaths = [];
  for (let index = 0; index < args.length; index += 2) {
    const option = args[index];
    const value = args[index + 1];
    if (!value || value.startsWith('--') || !['--compiled-exe', '--payload-exe', '--forbid-path'].includes(option)) {
      fail('Usage: node check-packaged-executable.mjs --compiled-exe <build-exe> --payload-exe <extracted-exe> [--forbid-path <root>] (repeatable), or --self-test.');
    }
    if (option === '--compiled-exe') {
      if (compiledFile) fail('Supply exactly one compiled executable.');
      compiledFile = value;
    } else if (option === '--payload-exe') {
      if (payloadFile) fail('Supply exactly one extracted executable.');
      payloadFile = value;
    } else {
      forbiddenPaths.push(value);
    }
  }
  if (!compiledFile || !payloadFile) fail('Supply both compiled and extracted executable files.');
  const lock = JSON.parse(fs.readFileSync(new URL('../../package-lock.json', import.meta.url), 'utf8'));
  const cliVersion = lock.packages['node_modules/@tauri-apps/cli']?.version;
  if (lock.packages['node_modules/@tauri-apps/cli-win32-x64-msvc']?.version !== reviewedCliVersion) {
    fail('Locked native Windows Tauri CLI version changed; review its binary-patching implementation before release.');
  }
  const compiled = fs.readFileSync(compiledFile);
  const payload = fs.readFileSync(payloadFile);
  const verification = verifyPackagedBytes(compiled, payload, cliVersion);
  const scanner = fileURLToPath(new URL('./check-release-privacy.mjs', import.meta.url));
  const scannerArgs = [scanner, '--file', payloadFile];
  for (const root of forbiddenPaths) scannerArgs.push('--forbid-path', root);
  const scan = spawnSync(process.execPath, scannerArgs, {
    cwd: projectRoot, encoding: 'utf8', maxBuffer: 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let privacy;
  try {
    privacy = JSON.parse(scan.stdout);
  } catch {
    fail('Extracted executable privacy scanner did not return a valid report; tool output is omitted.');
  }
  if (scan.status !== 0 || privacy.passed !== true ||
      privacy.sha256 !== verification.payloadExecutableSha256 ||
      privacy.bytes !== payload.length || !privacy.findingCounts ||
      Object.values(privacy.findingCounts).some((count) => count !== 0)) {
    fail('Extracted installer executable failed its direct privacy check; no review artifact is approved.');
  }
  process.stdout.write(`${JSON.stringify({
    ...verification,
    payloadPrivacyPassed: true,
    payloadPrivacyFindingCounts: privacy.findingCounts,
  })}\n`);
}

// Importing the pure verifier for independent synthetic checks has no CLI side effects.
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    if (error.safeDiagnostic) process.stdout.write(`${JSON.stringify(error.safeDiagnostic)}\n`);
    process.stderr.write(`${safeMessages.has(error.message) ? error.message : 'Packaged executable verification failed; local paths and tool output are omitted.'}\n`);
    process.exitCode = 1;
  }
}
