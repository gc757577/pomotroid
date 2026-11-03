export function localDay(date = new Date()): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}
export function clockRefresh(previousDay: string, previousTime: number, now = new Date()): boolean {
  const elapsed = now.getTime() - previousTime;
  return localDay(now) !== previousDay || elapsed > 5000 || elapsed < 0;
}
