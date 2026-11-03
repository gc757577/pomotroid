<script lang="ts">
  import '../../app.css';
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { getSettings, getThemes, onSettingsChanged, onThemesChanged, onRoundChange, onSessionsCleared, statsGetDetailed, statsGetHeatmap } from '$lib/ipc';
  import { settings } from '$lib/stores/settings';
  import { applyTheme } from '$lib/stores/theme';
  import { resolveThemeName } from '$lib/utils/theme';
  import { setLocale } from '$lib/locale.svelte.js';
  import { localDay, clockRefresh } from '$lib/utils/widget-refresh';
  import YearlyView from '$lib/components/stats/YearlyView.svelte';
  import type { DetailedStats, HeatmapStats } from '$lib/types';
  import * as m from '$paraglide/messages.js';

  interface Geometry { x: number; y: number; width: number; height: number }
  let detailed = $state<DetailedStats | null>(null);
  let heatmap = $state<HeatmapStats | null>(null);
  let day = $state(localDay());
  let error = $state('');
  let generation = 0;
  let disposed = false;
  let refreshing = false;
  let dirty = false;
  let clockDay = localDay();
  let clockTime = Date.now();
  let lastRefreshAttempt = Date.now();
  const action = (action: string, geometry?: Geometry, save = false) =>
    invoke<Geometry>('widget_action', { action, generation, geometry: geometry ?? null, save });

  async function refresh() {
    dirty = true;
    if (refreshing) return;
    refreshing = true;
    try {
      while (dirty && !disposed) {
        dirty = false;
        lastRefreshAttempt = Date.now();
        const [nextDetailed, nextHeatmap] = await Promise.all([statsGetDetailed(), statsGetHeatmap()]);
        if (!disposed) { detailed = nextDetailed; heatmap = nextHeatmap; day = localDay(); error = ''; }
      }
    } catch { if (!disposed) error = m.widget_refresh_failed(); }
    finally { refreshing = false; }
  }
  async function theme() {
    const themes = await getThemes();
    const dark = matchMedia('(prefers-color-scheme: dark)').matches;
    const selected = themes.find(t => t.name === resolveThemeName($settings, dark)) ?? themes[0];
    if (selected && !disposed) applyTheme(selected);
  }
  function fmtTime(mins: number) {
    if (mins < 60) return `${mins}m`;
    const hours = Math.floor(mins / 60), rest = mins % 60;
    return rest ? `${hours}h ${rest}m` : `${hours}h`;
  }

  let drag: { id: number; x: number; y: number; scale: number; resize: boolean; original?: Geometry; latest?: Geometry } | null = null;
  let pending: { geometry: Geometry; save: boolean } | null = null;
  let moving = false;
  async function flushGeometry() {
    if (moving) return;
    moving = true;
    try {
      while (pending && !disposed) {
        const next = pending; pending = null;
        await action('geometry', next.geometry, next.save);
      }
    } catch { if (!disposed) error = m.widget_position_failed(); }
    finally { moving = false; }
  }
  async function begin(event: PointerEvent, resize = false) {
    if (event.button !== 0 || drag) return;
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    const session = { id: event.pointerId, x: event.screenX, y: event.screenY, scale: devicePixelRatio, resize };
    drag = session;
    try { const original = await action('geometry'); if (drag === session) drag.original = original; }
    catch { drag = null; error = m.widget_position_failed(); }
  }
  function move(event: PointerEvent) {
    if (!drag?.original || drag.id !== event.pointerId) return;
    const dx = Math.round((event.screenX - drag.x) * drag.scale), dy = Math.round((event.screenY - drag.y) * drag.scale);
    const g = drag.original;
    drag.latest = drag.resize ? { ...g, width: Math.max(360, g.width + dx), height: Math.max(240, g.height + dy) } : { ...g, x: g.x + dx, y: g.y + dy };
    pending = { geometry: drag.latest, save: false }; void flushGeometry();
  }
  function end(event: PointerEvent, cancel = false) {
    if (!drag || drag.id !== event.pointerId) return;
    const g = cancel ? drag.original : drag.latest;
    if (g) { pending = { geometry: g, save: !cancel }; void flushGeometry(); }
    drag = null;
  }
  async function keyboard(event: KeyboardEvent, resize = false) {
    const delta: Record<string, [number, number]> = { ArrowLeft: [-10, 0], ArrowRight: [10, 0], ArrowUp: [0, -10], ArrowDown: [0, 10] };
    const d = delta[event.key]; if (!d) return; event.preventDefault();
    try {
      const g = await action('geometry');
      pending = { geometry: resize ? { ...g, width: Math.max(360, g.width + d[0]), height: Math.max(240, g.height + d[1]) } : { ...g, x: g.x + d[0], y: g.y + d[1] }, save: true };
      void flushGeometry();
    } catch { error = m.widget_position_failed(); }
  }

  onMount(() => {
    const cleanups: UnlistenFn[] = [];
    const add = async (promise: Promise<UnlistenFn>) => { const fn = await promise; if (disposed) fn(); else cleanups.push(fn); };
    const media = matchMedia('(prefers-color-scheme: dark)');
    const focusRefresh = () => { void refresh(); };
    const themeRefresh = () => { void theme().catch(() => {}); };
    generation = Number(new URLSearchParams(location.search).get('generation'));
    void (async () => {
      try {
        await Promise.all([
          add(onRoundChange(focusRefresh)), add(onSessionsCleared(focusRefresh)),
          add(listen('widget:refresh', focusRefresh)),
          add(onSettingsChanged(updated => { settings.set(updated); setLocale(updated.language); themeRefresh(); })),
          add(onThemesChanged(themeRefresh))
        ]);
        if (disposed) return;
        settings.set(await getSettings()); setLocale($settings.language);
        await theme(); await refresh();
      } catch { error = m.widget_refresh_failed(); }
      finally { if (!disposed) { try { await action('ready'); } catch { error = m.widget_refresh_failed(); } } }
    })();
    const clock = setInterval(() => {
      const now = new Date();
      if (clockRefresh(clockDay, clockTime, now) || now.getTime() - lastRefreshAttempt >= 60000) void refresh();
      clockDay = localDay(now); clockTime = now.getTime();
    }, 1000);
    window.addEventListener('focus', focusRefresh);
    document.addEventListener('visibilitychange', focusRefresh);
    media.addEventListener('change', themeRefresh);
    return () => {
      disposed = true; pending = null; clearInterval(clock); cleanups.forEach(fn => fn());
      window.removeEventListener('focus', focusRefresh); document.removeEventListener('visibilitychange', focusRefresh); media.removeEventListener('change', themeRefresh);
    };
  });
</script>

<svelte:head><title>Pomotroid · {m.stats_title()}</title></svelte:head>
<section class="widget">
  <header>
    <button class="drag" title={m.widget_move()} onpointerdown={e => begin(e)} onpointermove={move} onpointerup={e => end(e)} onpointercancel={e => end(e, true)} onkeydown={e => keyboard(e)}>
      <span>{m.stats_title()}</span><span class="date">{m.stats_tab_today()} · {day}</span>
    </button>
  </header>
  <div class="cards">
    <div><span class="label">{m.stats_rounds()}</span><strong>{detailed?.today.rounds ?? '—'}</strong></div>
    <div><span class="label">{m.stats_focus_time()}</span><strong>{detailed ? fmtTime(detailed.today.focus_mins) : '—'}</strong></div>
  </div>
  <div class="annual">{#key day}<YearlyView {heatmap} compact />{/key}</div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <button class="resize" aria-label={m.widget_resize()} title={m.widget_resize()} onpointerdown={e => begin(e, true)} onpointermove={move} onpointerup={e => end(e)} onpointercancel={e => end(e, true)} onkeydown={e => keyboard(e, true)}>◢</button>
</section>

<style>
  .widget { height: 100vh; display: flex; flex-direction: column; position: relative; overflow: hidden; box-sizing: border-box; background: var(--color-background); color: var(--color-foreground); border: 1px solid var(--color-separator); }
  header { flex-shrink: 0; border-bottom: 1px solid var(--color-separator); }
  .drag { width: 100%; display: flex; justify-content: space-between; gap: 12px; padding: 12px 16px; background: none; border: none; color: inherit; cursor: move; touch-action: none; user-select: none; font-size: 12px; font-weight: 600; }
  .date, .label { color: var(--color-foreground-darker); font-weight: 400; }
  .cards { display: flex; flex-shrink: 0; padding: 12px 16px 4px; gap: 28px; }
  .cards > div { display: flex; flex-direction: column; gap: 4px; min-width: 110px; }
  .label { font-size: 11px; } strong { font-size: 25px; font-variant-numeric: tabular-nums; }
  .annual { flex: 1; min-height: 0; overflow: hidden; }
  .resize { position: absolute; bottom: 0; right: 0; width: 25px; height: 25px; padding: 0; border: 0; background: none; color: var(--color-foreground-darker); cursor: nwse-resize; touch-action: none; }
  .error { font-size: 11px; padding: 2px 16px; color: var(--color-focus-round); margin: 0; }
</style>
