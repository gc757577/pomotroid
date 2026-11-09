import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import * as messages from '../src/paraglide/messages.js';
import { locales } from '../src/paraglide/runtime.js';

const readCatalog = async locale => JSON.parse(await readFile(new URL(`../src/messages/${locale}.json`, import.meta.url), 'utf8'));
const english = await readCatalog('en');
const korean = await readCatalog('ko');
const placeholders = text => [...text.matchAll(/\{(\w+)\}/g)].map(match => match[1]).sort();

test('Korean covers every message with matching interpolation inputs', () => {
  assert.ok(locales.includes('ko'));
  assert.deepEqual(Object.keys(korean).sort(), Object.keys(english).sort());
  for (const [key, text] of Object.entries(english)) {
    assert.ok(korean[key].trim(), key);
    assert.deepEqual(placeholders(korean[key]), placeholders(text), key);
  }
});

for (const locale of locales) {
  test(`${locale} generated messages preserve catalog text and base-language fallbacks`, async () => {
    const catalog = await readCatalog(locale);
    for (const [key, baseText] of Object.entries(english)) {
      const text = catalog[key] ?? baseText;
      const inputs = Object.fromEntries(placeholders(baseText).map(name => [name, 7]));
      const expected = text.replace(/\{(\w+)\}/g, (_, name) => String(inputs[name]));
      assert.equal(messages[key](inputs, { locale }), expected, `${locale}.${key}`);
    }
  });
}
