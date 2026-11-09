import { test } from 'node:test';
import assert from 'node:assert/strict';
import { remainingRatio, roundColor } from '../src/lib/utils/timer-progress.ts';

test('remaining bar shrinks from full to empty and clamps round boundaries', () => {
  assert.equal(remainingRatio(0, 100), 1);
  assert.equal(remainingRatio(25, 100), 0.75);
  assert.equal(remainingRatio(50, 100), 0.5);
  assert.equal(remainingRatio(100, 100), 0);
  assert.equal(remainingRatio(110, 100), 0);
  assert.equal(remainingRatio(-1, 100), 1);
  assert.equal(remainingRatio(0, 0), 1);
});

test('circle and horizontal bar use the same theme tokens for every round', () => {
  assert.equal(roundColor('work'), 'var(--color-focus-round)');
  assert.equal(roundColor('short-break'), 'var(--color-short-round)');
  assert.equal(roundColor('long-break'), 'var(--color-long-round)');
});
