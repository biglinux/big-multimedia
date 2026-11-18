(() => {
  'use strict';
  const pt = (navigator.language || '').toLowerCase().startsWith('pt');
  const words = pt ? {
    heading: 'Controle remoto', prev: 'Anterior', next: 'Próxima', play: 'Reproduzir', pause: 'Pausar',
    volume: 'Volume', progress: 'Progresso da reprodução', connecting: 'Conectando…',
    connected: 'Conectado ao computador', failed: 'Sem conexão. Verifique a rede e mantenha o player aberto.',
    warning: 'Use somente em uma rede local confiável. Esta conexão HTTP não é criptografada.'
  } : {
    heading: 'Remote control', prev: 'Previous', next: 'Next', play: 'Play', pause: 'Pause',
    volume: 'Volume', progress: 'Playback progress', connecting: 'Connecting…',
    connected: 'Connected to your computer', failed: 'Disconnected. Check the network and keep the player open.',
    warning: 'Use only on a trusted local network. This HTTP connection is not encrypted.'
  };
  const el = id => document.getElementById(id);
  document.documentElement.lang = pt ? 'pt-BR' : 'en';
  for (const id of ['heading', 'prev', 'next', 'warning']) el(id).textContent = words[id];
  el('volume-label').textContent = words.volume;
  el('media').setAttribute('aria-label', pt ? 'Mídia' : 'Media');
  el('progress').setAttribute('aria-label', words.progress);
  el('pp').textContent = words.play;
  el('connection').textContent = words.connecting;
  const base = new URL('./', window.location.href);
  let connected = false, busy = false, statusRequest = null, pollTimer = null;
  let volumeTimer = null, pendingVolume = null;
  const finite = value => Number.isFinite(value) ? Math.max(0, value) : 0;
  function time(value) { const n = Math.floor(finite(value)); return `${Math.floor(n/60)}:${String(n%60).padStart(2, '0')}`; }
  function controls() {
    for (const id of ['prev', 'next', 'pp']) el(id).disabled = !connected || busy;
    el('vol').disabled = !connected;
  }
  function connection(ok) {
    connected = ok; controls();
    const text = ok ? words.connected : words.failed;
    if (el('connection').textContent !== text) el('connection').textContent = text;
  }
  async function request(path, method, controller) {
    const deadline = setTimeout(() => controller.abort(), 4000);
    try {
      const result = await fetch(new URL(path, base), {
        method, headers: {'X-Big-Media-Remote':'1'}, cache:'no-store', redirect:'error',
        credentials:'omit', referrerPolicy:'no-referrer', signal:controller.signal
      });
      if (!result.ok) throw new Error('request rejected');
      return method === 'GET' ? await result.json() : await result.text();
    } finally { clearTimeout(deadline); }
  }
  async function refresh() {
    if (document.hidden || statusRequest) return;
    statusRequest = new AbortController();
    try {
      const value = await request('status', 'GET', statusRequest);
      if (document.hidden) return;
      el('title').textContent = typeof value.title === 'string' ? value.title : '—';
      el('artist').textContent = typeof value.artist === 'string' ? value.artist : '';
      el('pos').textContent = time(value.position); el('dur').textContent = time(value.duration);
      el('progress').max = finite(value.duration) || 1;
      el('progress').value = Math.min(finite(value.position), el('progress').max);
      el('pp').textContent = value.playing ? words.pause : words.play;
      if (pendingVolume === null && !busy && document.activeElement !== el('vol')) {
        el('vol').value = Math.min(100, Math.round(finite(value.volume)));
        el('volume-value').textContent = `${el('vol').value}%`;
      }
      connection(true);
    } catch (_) { if (!document.hidden) connection(false); }
    finally {
      statusRequest = null;
      if (!document.hidden) { clearTimeout(pollTimer); pollTimer = setTimeout(refresh, 1000); }
    }
  }
  async function act(path) {
    if (busy || !connected || document.hidden) return;
    busy = true; controls();
    try { await request(path, 'POST', new AbortController()); }
    catch (_) { connection(false); }
    finally {
      busy = false; controls();
      if (pendingVolume !== null) scheduleVolume();
      else { clearTimeout(pollTimer); pollTimer = setTimeout(refresh, 80); }
    }
  }
  function scheduleVolume() {
    clearTimeout(volumeTimer);
    if (!connected || document.hidden) { pendingVolume = null; return; }
    volumeTimer = setTimeout(() => {
      if (busy) { scheduleVolume(); return; }
      const value = pendingVolume; pendingVolume = null;
      if (value !== null) void act(`volume?v=${value}`);
    }, 150);
  }
  el('prev').addEventListener('click', () => void act('prev'));
  el('next').addEventListener('click', () => void act('next'));
  el('pp').addEventListener('click', () => void act('play-pause'));
  el('vol').addEventListener('input', () => {
    pendingVolume = Math.min(100, Math.max(0, Math.round(Number(el('vol').value))));
    el('volume-value').textContent = `${pendingVolume}%`; scheduleVolume();
  });
  document.addEventListener('visibilitychange', () => {
    clearTimeout(pollTimer); clearTimeout(volumeTimer); pendingVolume = null;
    if (document.hidden) { if (statusRequest) statusRequest.abort(); }
    else void refresh();
  });
  void refresh();
})();
