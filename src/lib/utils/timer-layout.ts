// Mini is fixed at 88 logical client pixels high. Normal windows are at least
// 240 outer logical pixels high; the same 160px cutoff is used by the CSS views.
export function normalTimerLayout(width: number, height: number) {
  if (height <= 160) return null;
  const isCompact = width < 300 || height < 300;
  const uiScale = isCompact
    ? Math.max(0.4, Math.min(Math.min(width - 16, height - 40 - 16 - 48) / 220, 4))
    : Math.max(0.5, Math.min(width / 360, (height - 40) / (478 - 40), 4));
  return { isCompact, uiScale };
}
