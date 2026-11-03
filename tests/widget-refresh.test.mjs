import test from 'node:test';
import assert from 'node:assert/strict';
import { localDay, clockRefresh } from '../src/lib/utils/widget-refresh.ts';

test('local midnight and new year refresh the calendar', () => {
  const before = new Date(2026, 11, 31, 23, 59, 59);
  const after = new Date(2027, 0, 1, 0, 0, 0);
  assert.equal(localDay(before), '2026-12-31');
  assert.equal(clockRefresh(localDay(before), before.getTime(), after), true);
});
test('a suspended or throttled clock refreshes even on the same day', () => {
  const before = new Date(2026, 10, 1, 10, 0, 0);
  const after = new Date(before.getTime() + 15 * 60 * 1000);
  assert.equal(clockRefresh(localDay(before), before.getTime(), after), true);
});
test('ordinary polls do not query statistics every second', () => {
  const before = new Date(2026, 10, 1, 10, 0, 0);
  assert.equal(clockRefresh(localDay(before), before.getTime(), new Date(before.getTime() + 1000)), false);
});
test('moving the system clock backwards also refreshes', () => {
  const before = new Date(2026, 10, 1, 10, 0, 0);
  assert.equal(clockRefresh(localDay(before), before.getTime(), new Date(before.getTime() - 1000)), true);
});
