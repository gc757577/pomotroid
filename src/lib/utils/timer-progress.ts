export function remainingRatio(elapsed: number, total: number): number {
  return total > 0 ? Math.max(0, Math.min(1, 1 - elapsed / total)) : 1;
}

export function roundColor(roundType: string): string {
  if (roundType === 'work') return 'var(--color-focus-round)';
  if (roundType === 'short-break') return 'var(--color-short-round)';
  return 'var(--color-long-round)';
}
