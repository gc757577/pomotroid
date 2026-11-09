import { test } from 'node:test';
import assert from 'node:assert/strict';
import { normalTimerLayout } from '../src/lib/utils/timer-layout.ts';

test('prepare normal layout before expansion without using mini viewport dimensions', () => {
  const normal = normalTimerLayout(360, 478);
  assert.deepEqual(normal, { isCompact: false, uiScale: 1 });
  assert.equal(normalTimerLayout(200, 88), null);
  assert.equal(normalTimerLayout(0, 0), null);
  assert.deepEqual(normalTimerLayout(360, 478), normal);
});

test('small and large normal layouts preserve the original uniform scale rules', () => {
  const small = normalTimerLayout(224, 231);
  assert.equal(small.isCompact, true);
  assert.equal(small.uiScale, 127 / 220);
  const large = normalTimerLayout(900, 700);
  assert.equal(large.isCompact, false);
  assert.equal(large.uiScale, 660 / 438);
});
