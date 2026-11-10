import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

test('requested first-run defaults agree across DB, Rust fallback and frontend', async () => {
  const read = path => readFile(new URL(`../${path}`, import.meta.url), 'utf8');
  const [db, rustSource, frontend] = await Promise.all([
    read('src-tauri/src/settings/defaults.rs'),
    read('src-tauri/src/settings/mod.rs'),
    read('src/lib/stores/settings.ts'),
  ]);
  const rust = rustSource.split('impl Default for Settings')[1].split('/// Seed')[0];
  const expected = {
    widget_enabled: false, widget_opacity_percent: 67, timer_opacity_percent: 54,
    hover_opacity_enabled: true, hover_opacity_percent: 80,
    autostart_enabled: false, websocket_enabled: false, language: 'auto',
    verbose_logging: false, tray_icon_enabled: true, min_to_tray: false,
    min_to_tray_on_close: true, always_on_top: true, break_always_on_top: false,
    // Previously inconsistent frontend placeholders follow effective DB defaults.
    auto_start_work: true, auto_start_break: true, theme_light: 'Pomotroid Light',
  };
  for (const [key, value] of Object.entries(expected)) {
    assert.ok(db.includes(`("${key}", "${value}")`), `DB: ${key}`);
    const rustValue = typeof value === 'string' ? `"${value}".to_string()` : String(value);
    const frontendValue = typeof value === 'string' ? `'${value}'` : String(value);
    assert.ok(rust.includes(`${key}: ${rustValue},`), `Rust: ${key}`);
    assert.ok(frontend.includes(`${key}: ${frontendValue},`), `frontend: ${key}`);
  }
});
