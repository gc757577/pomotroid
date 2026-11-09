import { overwriteGetLocale, baseLocale, locales } from '$paraglide/runtime.js';
import { resolveLocale } from '$lib/utils/locale';
import { getName } from '@tauri-apps/api/app';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import * as m from '$paraglide/messages.js';
import { info } from '@tauri-apps/plugin-log';

type SupportedLocale = (typeof locales)[number];

// Module-level reactive state (Svelte 5 rune).
// Reading this inside any Svelte template expression — even through a function
// call chain like m.nav_timer() → getLocale() → currentLocale — creates a
// tracked dependency. When currentLocale changes, all those expressions
// automatically re-evaluate without a page reload.
let currentLocale: SupportedLocale = $state(baseLocale as SupportedLocale);

// Override Paraglide's getLocale so that every m.*() call reads from our
// reactive $state instead of its internal plain-JS variable.
overwriteGetLocale(() => currentLocale);

/**
 * Change the active locale. Call this instead of Paraglide's setLocale.
 * Accepts the raw DB value ('auto' | BCP-47 tag) and resolves it.
 */
export function setLocale(language: string): void {
  const resolved = resolveLocale(language) as SupportedLocale;
  currentLocale = resolved;
  document.documentElement.lang = resolved;
  const window = getCurrentWindow();
  const title = window.label === 'settings' ? m.settings_title() : window.label === 'stats' ? m.stats_title() : null;
  if (title) {
    void getName().then(name => {
      const currentTitle = window.label === 'settings' ? m.settings_title() : m.stats_title();
      document.title = `${name} — ${currentTitle}`;
      return window.setTitle(document.title);
    }).catch(error => console.error('[locale] window title', error));
  }
  void invoke('tray_set_labels', { labels: {
    start: m.tray_start(), pause: m.tray_pause(), resume: m.tray_resume(),
    skip: m.tray_skip(), reset_round: m.tray_reset_round(), show: m.tray_show(), exit: m.tray_exit(),
  } }).catch(error => console.error('[locale] tray labels', error));
  info(`[locale] locale set to ${resolved}`).catch(() => {});
}
