import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const read = async path => JSON.parse(await readFile(new URL(`../${path}`, import.meta.url), 'utf8'));
const config = await read('src-tauri/tauri.conf.json');
const dev = await read('src-tauri/tauri.dev.conf.json');

test('preview cannot share original/development installation or Tauri data identity', () => {
  assert.equal(config.identifier, 'io.github.gc75.pomotroidfork.preview');
  assert.equal(new Set([config.identifier, dev.identifier, 'com.splode.pomotroid']).size, 3);
  assert.equal(config.productName, 'ClearPomo');
  assert.notEqual(config.productName, dev.productName);
  assert.equal(config.mainBinaryName, 'pomotroid-fork-preview');
  assert.equal(config.bundle.windows.nsis.installMode, 'currentUser');
  assert.deepEqual(config.bundle.targets, ['nsis']);
});

test('ClearPomo display rename retains Preview installer and autostart identities', async () => {
  const template = await readFile(new URL('../src-tauri/windows/installer.nsi', import.meta.url), 'utf8');
  const startup = await readFile(new URL('../src-tauri/src/startup.rs', import.meta.url), 'utf8');
  assert.equal(config.bundle.windows.nsis.template, 'windows/installer.nsi');
  assert.equal(config.bundle.windows.nsis.startMenuFolder, 'ClearPomo');
  assert.match(template, /!define INSTALLIDENTITY "Pomotroid Fork Preview"/);
  assert.match(template, /Uninstall\\\$\{INSTALLIDENTITY\}/);
  assert.match(template, /!define MANUPRODUCTKEY "\$\{MANUKEY\}\\\$\{INSTALLIDENTITY\}"/);
  assert.match(template, /WriteRegStr SHCTX "\$\{UNINSTKEY\}" "DisplayName" "\$\{PRODUCTNAME\}"/);
  assert.match(startup, /Uninstall\\Pomotroid Fork Preview/);
  assert.equal((await read('node_modules/@tauri-apps/cli/package.json')).version, '2.11.2', 'Review the vendored NSIS template when upgrading CLI');
});

test('preview bundles only declared license resources and no updater endpoint', async () => {
  assert.deepEqual(config.bundle.resources, { 'target/preview-resources/': 'licenses/' });
  assert.equal(config.bundle.licenseFile, '../LICENSE');
  assert.equal(config.plugins.updater, undefined);
  assert.equal(dev.plugins?.updater, undefined);
  const capabilities = await read('src-tauri/capabilities/default.json');
  assert.ok(!capabilities.permissions.some(permission => permission.startsWith('updater:')));
  assert.equal(config.version, (await read('package.json')).version);
  assert.equal(config.version, (await read('package-lock.json')).version);
});
