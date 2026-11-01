<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';

  interface Report { mode: string; message: string; clicks: number; generation: number; ready: boolean }
  let report = $state<Report>({ mode: 'loading', message: 'WebView 연결 중', clicks: 0, generation: 0, ready: false });
  let error = $state('');
  let busy = $state(false);
  async function act(action: string) {
    busy = true;
    try { report = await invoke<Report>('desktop_probe_action', { action }); error = ''; }
    catch (e) { error = String(e); }
    finally { busy = false; }
  }
  onMount(() => {
    const unlisten = listen<Report>('desktop-probe:status', e => { report = e.payload; });
    void act('ready');
    return () => { void unlisten.then(fn => fn()); };
  });
</script>

<svelte:head><title>Desktop WebView Probe</title></svelte:head>
<main>
  <h1>Tauri WebView 시험 창</h1>
  <p>상태: <strong>{report.mode}</strong> · 생성 #{report.generation}</p>
  <p aria-live="polite">클릭 횟수: <strong>{report.clicks}</strong></p>
  <div>
    <button disabled={busy} onclick={() => act('click')}>Click test</button>
    <button disabled={busy || !report.ready} onclick={() => act('attach')}>바탕화면 부착</button>
    <button disabled={busy} onclick={() => act('detach')}>일반 창으로 복구</button>
  </div>
  <p>{report.message}</p>
  {#if error}<p role="alert">{error}</p>{/if}
</main>

<style>
  :global(html), :global(body) { margin: 0; background: #ffffff; color: #111111; }
  main { padding: 16px; font-family: sans-serif; font-size: 14px; }
  h1 { font-size: 18px; margin: 0 0 12px; }
  p { margin: 10px 0; }
  div { display: flex; gap: 6px; flex-wrap: wrap; }
  button { padding: 8px; color: #111; background: #eee; border: 1px solid #777; border-radius: 3px; }
  [role='alert'] { color: #a00; }
</style>
