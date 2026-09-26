import { mountSoundUI, renderSoundUI } from './sound-ui.js';
import { stateInfo, remaining, clock, batteryPresentation, powerPresentation, soundPresentation } from './view-model.js';
const $ = id => document.getElementById(id);
const nativeInvoke = window.__TAURI__?.core?.invoke;
let view = null, initialized = false, polling = false, toastTimer;
const text = (id, value) => { $(id).textContent = value ?? '—'; };
function notify(message, error = false) {
  clearTimeout(toastTimer); const node = $('toast'); node.textContent = String(message); node.classList.toggle('error', error); node.hidden = false;
  toastTimer = setTimeout(() => { node.hidden = true; }, error ? 8500 : 6000);
}
async function invoke(name, args = {}) {
  if (!nativeInvoke) throw new Error('当前是静态界面。请通过 Windows 编译后的 AI Light 打开。');
  return nativeInvoke(name, args);
}
async function run(action) { try { await action(); await refresh(); } catch (e) { notify(e?.message ?? String(e), true); } }
function selectPage(name) {
  const page = $(`page-${name}`); if (!page) return;
  document.querySelectorAll('.page').forEach(p => { p.hidden = p !== page; });
  document.querySelectorAll('.nav').forEach(b => b.classList.toggle('active', b.dataset.page === name));
  text('page-title', { overview: '状态总览', effects: '灯效与规则', sound: '声音提醒', device: '设备与电池', connect: 'Codex 接入', events: '事件记录', preferences: '偏好设置' }[name]);
  document.querySelector('.content').scrollTop = 0;
}
function theme(value) {
  const selected = value === 'system' ? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light') : value;
  document.documentElement.dataset.theme = selected;
}
function initFields(v) {
  const c = v.config;
  $('period').value = c.period_ms; $('fade').value = c.fade_ms; updateSliders();
  $('done-seconds').value = c.done_seconds; $('serial').value = c.serial; $('battery-poll').value = c.battery_poll_seconds; $('voltage-enabled').checked = c.voltage_enabled === true;
  $('bind-host').value = c.bind_host; $('port').value = c.port; $('allowed-sources').value = c.allowed_sources.join(', ');
  $('autostart').checked = v.autostart; $('exit-off').checked = c.exit_off; $('reduce-motion').checked = c.reduce_motion; $('theme').value = c.theme;
}
function updateSliders() {
  const period = Number($('period').value);
  $('fade').max = Math.floor(period / 200) * 100;
  if (Number($('fade').value) > Number($('fade').max)) $('fade').value = $('fade').max;
  text('period-value', `${(period / 1000).toFixed(1)} 秒`); text('fade-value', `${(Number($('fade').value) / 1000).toFixed(1)} 秒`);
}
function events(container, entries) {
  container.replaceChildren();
  if (!entries.length) { const empty = document.createElement('div'); empty.className = 'empty'; empty.textContent = '尚未收到事件'; container.append(empty); return; }
  const labels = { state: '任务状态', hardware: '设备', network: '接收端', lifecycle: '运行', preview: '灯效测试', settings: '设置', export: '导出', output: '灯光输出', power: '电压', sound: '声音提醒', battery: '电量保护' };
  for (const event of entries) {
    const row = document.createElement('div'); row.className = 'event-row';
    for (const [className, value] of [['event-time', clock(event.at_ms)], ['event-kind', labels[event.kind] || event.kind], ['event-message', event.message]]) {
      const cell = document.createElement('span'); cell.className = className; cell.textContent = value; row.append(cell);
    } container.append(row);
  }
}
function sources(v) {
  const node = $('sources-list'); node.replaceChildren();
  for (const source of v.aggregate.sources) {
    const row = document.createElement('div'); row.className = 'detail-row';
    const left = document.createElement('span'); left.textContent = source.id;
    const right = document.createElement('span'); right.textContent = `${source.online ? '在线' : '失联'} · ${source.sessions} 个会话 · ${clock(source.last_seen_ms)}`;
    row.append(left, right); node.append(row);
  }
  if (!node.children.length) { const empty = document.createElement('div'); empty.className = 'empty'; empty.textContent = '尚无来源；转发器默认每 15 秒发送一次心跳。'; node.append(empty); }
}
function render(v) {
  view = v; if (!initialized) { initFields(v); initialized = true; }
  theme(v.config.theme); document.body.classList.toggle('reduced-motion', v.config.reduce_motion);
  document.documentElement.style.setProperty('--breath-period', `${v.config.period_ms / 1000}s`);
  text('version', `v${v.version}`);
  const info = stateInfo[v.output] || stateInfo.off;
  $('hero').dataset.state = v.output;
  text('state-label', v.paused ? '灯光已暂停。' : info.title);
  text('state-description', v.paused ? '后台继续接收最新状态，恢复后重新跟随任务。' : info.description);
  text('effect-label', info.effect); text('mode-label', v.paused ? '输出暂停' : v.preview ? '灯效预览' : '自动模式');
  text('pause', v.paused ? '恢复灯光' : '暂停灯光');
  const countdown = v.preview ? `${remaining(v.preview.until_ms - v.now_ms)} 后结束预览` : v.output === 'done' && v.aggregate.remaining_ms != null ? `${remaining(v.aggregate.remaining_ms)} 后熄灭` : '';
  text('countdown', countdown); $('cancel-preview').disabled = !v.preview;
  text('preview-hint', v.preview ? '正在测试；新的等待/异常事件会自动结束预览。' : '每种灯效测试 10 秒，然后恢复最新自动状态。');
  text('timer-test', `测试 ${Math.round(v.config.done_seconds / 60)} 分钟熄灯`);
  const h = v.hardware;
  text('device-summary', h.present === true ? '已发现接口' : h.present === false ? '未发现设备' : '正在检查');
  text('device-caption', h.device_name || `BLE HID · ${v.config.serial.toUpperCase()}`);
  text('device-name', h.device_name || 'YY-AiLight');
  text('device-presence', h.present === true ? '发现指定设备' : h.present === false ? '未发现' : '检查中');
  text('output-status', h.api_success === true ? `调用成功 · ${clock(h.last_output_ms)}` : h.api_success === false ? '调用失败' : '尚未输出');
  text('ack-status', h.acknowledged === true ? '收到有效 LED 回执' : h.acknowledged === false ? '设备返回错误' : '未取得有效协议确认');
  text('device-error', h.error || '');
  const b = batteryPresentation(v.battery), p = powerPresentation(v.power), audio = soundPresentation(v.sound, v.now_ms);
  text('battery-summary', b.summary); text('battery-old', b.suffix); text('battery-guard-summary', audio.low ? '低电量 · 蜂鸣暂停' : b.old ? '历史电量，不用于声音判断' : '标准 BLE 电量');
  text('voltage-summary', p.voltageShort); text('device-voltage', p.voltage); text('power-source', p.source);
  text('power-updated', v.power?.updated_ms ? `${clock(v.power.updated_ms)}${p.old ? ' · 历史读数' : ''}` : '尚未读取');
  text('power-raw-frame', p.raw); text('power-error', v.power?.error || '');
  $('battery-fill').style.width = `${b.percent ?? 0}%`; $('battery-fill').classList.toggle('stale', b.old); $('battery-fill').classList.toggle('low', b.low);
  text('device-percent', b.number); text('device-percent-unit', b.valid ? (b.old ? '% · 上次' : '%') : '');
  text('device-low-status', audio.low ? (b.old ? '上次低电量' : '低电量保护') : b.valid && !b.old ? '有效电量读数' : '等待有效电量');
  $('device-low-status').classList.toggle('low', audio.low); text('battery-sound-status', audio.guard);
  text('battery-updated', v.battery.updated_ms ? `${clock(v.battery.updated_ms)}${b.old ? ' · 已过期' : ''}` : '尚未读取');
  text('battery-note', b.note); text('battery-error', v.battery.error || '');
  $('battery-warning').hidden = !audio.low;
  text('battery-warning', `低电量保护已启用${b.valid ? ` · ${b.summary}${b.old ? '（上次读数）' : ''}` : ''}。蜂鸣和试听已暂停，灯光继续按任务运行；连续两次 ≥25% 后解除，不补播旧声音。`);
  text('sound-overview', audio.title); text('sound-overview-detail', audio.state ? `${audio.state} · ${audio.reason}` : audio.reason);
  renderSoundUI(v);
  const online = v.aggregate.sources.filter(s => s.online).length;
  text('source-summary', online ? `${online} 个来源在线` : '等待接入');
  text('source-caption', v.aggregate.sources.length ? `${v.aggregate.sessions.length} 个会话 · 15 秒心跳` : '没有收到状态心跳');
  text('receiver-address', v.receiver.address); text('receiver-status', v.receiver.listening ? '正在监听' : '未启动');
  text('receiver-error', v.receiver.error || '');
  $('sidebar-dot').classList.toggle('online', v.receiver.listening); text('sidebar-label', v.receiver.listening ? '后台正在运行' : '接收端未就绪');
  sources(v); events($('recent-events'), v.logs.slice(0, 3)); events($('events-list'), v.logs);
}
async function refresh() {
  if (polling || !nativeInvoke) return;
  polling = true;
  try { render(await invoke('get_view')); } catch (e) { text('sidebar-label', '本地连接中断'); $('sidebar-dot').classList.remove('online'); } finally { polling = false; }
}
async function save(partial) {
  if (!view) throw new Error('程序尚未就绪');
  await invoke('save_settings', { settings: { ...view.config, ...partial } });
  notify('设置已保存。');
}
document.querySelectorAll('[data-page]').forEach(b => b.addEventListener('click', () => selectPage(b.dataset.page)));
document.querySelectorAll('[data-test]').forEach(b => b.addEventListener('click', () => run(() => invoke('set_preview', { state: b.dataset.test, seconds: 10 }))));
$('pause').addEventListener('click', () => run(() => invoke('set_paused', { paused: !view?.paused })));
$('cancel-preview').addEventListener('click', () => run(() => invoke('clear_preview')));
$('timer-test').addEventListener('click', () => run(() => invoke('set_preview', { state: 'done', seconds: view?.config.done_seconds || 300 })));
$('period').addEventListener('input', updateSliders); $('fade').addEventListener('input', updateSliders);
$('save-effects').addEventListener('click', () => run(() => save({ period_ms: Number($('period').value), fade_ms: Number($('fade').value), done_seconds: Number($('done-seconds').value) })));
$('scan-devices').addEventListener('click', () => run(async () => {
  const found = await invoke('find_devices'); const select = $('discovered-devices');
  select.replaceChildren(); select.hidden = found.length === 0;
  if (!found.length) { notify('没有发现兼容接口；先在 Windows 蓝牙设置中配对并打开灯。'); return; }
  const prompt = document.createElement('option'); prompt.value = ''; prompt.textContent = '选择设备，然后点击保存'; select.append(prompt);
  for (const d of found) { const option = document.createElement('option'); option.value = d.serial; option.textContent = `${d.name} · ${d.serial.toUpperCase()}`; select.append(option); }
  if (found.length === 1) { select.value = found[0].serial; $('serial').value = found[0].serial; }
  notify(`发现 ${found.length} 台兼容设备；确认后点击保存设备设置。`);
}));
$('discovered-devices').addEventListener('change', () => { if ($('discovered-devices').value) $('serial').value = $('discovered-devices').value; });
$('save-device').addEventListener('click', () => run(() => save({ serial: $('serial').value.trim(), battery_poll_seconds: Number($('battery-poll').value), voltage_enabled: $('voltage-enabled').checked })));
$('save-network').addEventListener('click', () => run(() => save({ bind_host: $('bind-host').value.trim(), port: Number($('port').value), allowed_sources: $('allowed-sources').value.split(',').map(x => x.trim()).filter(Boolean) })));
$('save-preferences').addEventListener('click', () => run(() => save({ exit_off: $('exit-off').checked, reduce_motion: $('reduce-motion').checked, theme: $('theme').value })));
$('autostart').addEventListener('change', () => run(async () => {
  try { await invoke('set_autostart', { enabled: $('autostart').checked }); notify('登录自启设置已更新。'); } catch (e) { $('autostart').checked = ! $('autostart').checked; throw e; }
}));
$('theme-toggle').addEventListener('click', () => run(async () => { const value = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark'; await save({ theme: value }); $('theme').value = value; }));
$('reconnect').addEventListener('click', () => run(async () => { await invoke('refresh_device'); notify('已请求重新发现设备并刷新电量。'); }));
$('battery-refresh').addEventListener('click', () => run(async () => { await invoke('refresh_battery'); notify('已请求刷新 BLE 电量与 HID 电压。'); }));
$('bluetooth').addEventListener('click', () => run(() => invoke('bluetooth_settings')));
$('export-client').addEventListener('click', () => run(async () => { const path = await invoke('export_client', { host: $('host-ip').value.trim(), sourceId: $('source-id').value.trim() }); notify(`已保存：${path}。文件含访问密钥，请妥善保管。`); await invoke('open_folder', { kind: 'exports' }); }));
$('open-exports').addEventListener('click', () => run(() => invoke('open_folder', { kind: 'exports' })));
$('open-data').addEventListener('click', () => run(() => invoke('open_folder', { kind: 'data' })));
$('export-diagnostics').addEventListener('click', () => run(async () => { const path = await invoke('export_diagnostics'); notify(`已保存诊断：${path}`); await invoke('open_folder', { kind: 'exports' }); }));
$('stop-sound-overview').addEventListener('click', () => run(() => invoke('stop_sound')));
mountSoundUI({ invoke, run, notify, save, getView: () => view });
$('quit').addEventListener('click', () => run(() => invoke('quit')));
document.addEventListener('visibilitychange', () => { document.body.classList.toggle('document-hidden', document.hidden); if (!document.hidden) refresh(); });
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => { if (view) theme(view.config.theme); });
if (!nativeInvoke) { $('runtime-banner').hidden = false; text('state-label', '界面未连接程序'); text('state-description', '请在编译后的 AI Light 中使用真实灯控功能。'); text('sidebar-label', '静态界面'); }
async function tick() { await refresh(); setTimeout(tick, document.hidden ? 4000 : 1000); }
tick();
