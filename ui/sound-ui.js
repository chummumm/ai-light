import { soundStates, soundNames, modeNames, soundDefaults, rawSoundVolume, validateSoundConfig, soundPresentation } from './view-model.js';
const $ = id => document.getElementById(id);
let dirty = false, applied = '', api;
function markDirty() {
  dirty = true; $('sound-dirty').textContent = '有未保存的声音规则';
  document.querySelector('.sound-save-bar').classList.add('dirty');
}
function id(key, name) { return $(`sound-${key}-${name}`); }
function updateRule(key) {
  const on = id(key, 'enabled').checked;
  $(`sound-rule-${key}`).dataset.disabled = String(!on);
  const inherit = id(key, 'inherit').checked;
  const volume = inherit ? Number($('sound-default-volume').value) : Number(id(key, 'volume').value);
  id(key, 'volume').disabled = inherit;
  id(key, 'volume-value').textContent = `${volume}%`;
  id(key, 'raw').textContent = `原始驱动 ${rawSoundVolume(volume)} / 100`;
  const kind = id(key, 'limit').value;
  id(key, 'limit-field').hidden = ['once', 'until_state'].includes(kind);
  id(key, 'interval-field').hidden = kind === 'once';
  id(key, 'limit-label').textContent = kind === 'count' ? '最多响几轮' : '总提醒时长（秒）';
  id(key, 'limit-value').min = kind === 'count' ? '1' : '5';
  id(key, 'limit-value').max = kind === 'count' ? '1000' : '3600';
}
function updateDefault() {
  const volume = Number($('sound-default-volume').value);
  $('sound-default-value').textContent = `${volume}%`;
  $('sound-default-raw').textContent = `写入原始驱动 ${rawSoundVolume(volume)} / 100。100% 档位对应原始值 50；仅在目标值改变时写入设备。`;
  soundStates.forEach(updateRule);
}
function load(config) {
  $('sound-default-volume').value = config.default_volume;
  for (const key of soundStates) {
    const r = config[key];
    id(key, 'enabled').checked = r.enabled; id(key, 'mode').value = r.mode;
    id(key, 'inherit').checked = r.volume == null; id(key, 'volume').value = r.volume ?? config.default_volume;
    id(key, 'delay').value = r.delay_seconds; id(key, 'interval').value = r.interval_seconds;
    id(key, 'limit').value = r.limit.kind; id(key, 'limit-value').value = r.limit.seconds ?? r.limit.count ?? 30;
  }
  updateDefault();
}
function collect() {
  const config = { enabled: api.getView()?.config.sound.enabled ?? true, default_volume: Number($('sound-default-volume').value) };
  for (const key of soundStates) {
    const kind = id(key, 'limit').value, number = Number(id(key, 'limit-value').value);
    const limit = kind === 'duration' ? { kind, seconds: number } : kind === 'count' ? { kind, count: number } : { kind };
    config[key] = { enabled: id(key, 'enabled').checked, mode: id(key, 'mode').value,
      volume: id(key, 'inherit').checked ? null : Number(id(key, 'volume').value),
      delay_seconds: Number(id(key, 'delay').value), interval_seconds: Number(id(key, 'interval').value), limit };
  }
  return validateSoundConfig(config);
}
export function mountSoundUI(actions) {
  api = actions;
  for (const key of soundStates) {
    const card = document.createElement('article'); card.className = 'sound-rule'; card.id = `sound-rule-${key}`;
    const hint = { waiting: '直到你回来，按节奏提醒', error: '留一点时间给自动重试', done: '轻一点，完成不必一直响', working: '默认不响，可以单独开启' }[key];
    // All interpolated values are local enums, never remote/user text.
    card.innerHTML = `<header class="sound-rule-header"><div><h3>${soundNames[key]}</h3><p>${hint}</p></div><input class="switch" type="checkbox" id="sound-${key}-enabled" aria-label="启用${soundNames[key]}声音"></header>
      <div class="rule-fields">
        <div class="field"><label for="sound-${key}-mode">声音模式</label><select id="sound-${key}-mode">${Object.entries(modeNames).map(([k,v]) => `<option value="${k}">${v}</option>`).join('')}</select></div>
        <div class="field"><label for="sound-${key}-delay">首次延迟（秒）</label><input id="sound-${key}-delay" type="number" min="0" max="300" value="0"></div>
        <div class="field span-2"><label class="inherit-label"><input id="sound-${key}-inherit" type="checkbox">使用默认音量</label><div class="range-row"><input id="sound-${key}-volume" type="range" min="0" max="100" value="70"><output id="sound-${key}-volume-value">70%</output></div></div>
        <div class="field span-2"><label for="sound-${key}-limit">播放 / 停止方式</label><select id="sound-${key}-limit"><option value="once">只响一轮</option><option value="duration">重复至指定时长</option><option value="count">重复至指定次数</option><option value="until_state">重复至状态结束 / 手动停止</option></select></div>
        <div class="field" id="sound-${key}-interval-field"><label for="sound-${key}-interval">重复间隔（秒）</label><input id="sound-${key}-interval" type="number" min="5" max="3600" value="15"></div>
        <div class="field" id="sound-${key}-limit-field"><label id="sound-${key}-limit-label" for="sound-${key}-limit-value">总时长（秒）</label><input id="sound-${key}-limit-value" type="number" min="5" max="3600" value="30"></div>
      </div><footer class="rule-footer"><small id="sound-${key}-raw"></small><button class="text-button" id="sound-${key}-test">试听一轮</button></footer>`;
    $('sound-rules').append(card);
    card.querySelectorAll('input,select').forEach(node => node.addEventListener('input', () => { markDirty(); updateRule(key); }));
    id(key, 'limit').addEventListener('change', () => {
      if (id(key, 'limit').value === 'count') id(key, 'limit-value').value = '3';
      else if (id(key, 'limit').value === 'duration') id(key, 'limit-value').value = '30';
      updateRule(key); markDirty();
    });
    id(key, 'test').addEventListener('click', () => api.run(async () => {
      const cfg = collect(), rule = cfg[key], volume = rule.volume ?? cfg.default_volume;
      if (volume === 0) throw new Error('当前试听音量为 0，请先调高');
      await api.invoke('test_sound', { mode: rule.mode, volume });
      api.notify('已请求试听一轮。受电量保护；不改变任务灯光。');
    }));
  }
  load(soundDefaults());
  $('sound-default-volume').addEventListener('input', () => { markDirty(); updateDefault(); });
  $('sound-enabled').addEventListener('change', () => api.run(async () => {
    const desired = $('sound-enabled').checked; $('sound-enabled').disabled = true;
    try { await api.invoke('set_sound_enabled', { enabled: desired }); api.notify(desired ? '蜂鸣已开启；不补播旧提醒。' : '蜂鸣已关闭。'); }
    finally { $('sound-enabled').disabled = false; }
  }));
  $('stop-sound').addEventListener('click', () => api.run(() => api.invoke('stop_sound')));
  $('save-sound').addEventListener('click', () => api.run(async () => {
    const sound = collect(); await api.save({ sound });
    dirty = false; applied = ''; $('sound-dirty').textContent = '声音规则已保存';
    document.querySelector('.sound-save-bar').classList.remove('dirty');
  }));
  $('reset-sound').addEventListener('click', () => { load(soundDefaults()); markDirty(); api.notify('已填入默认规则；点击保存后生效。'); });
}
export function renderSoundUI(view) {
  const config = view.config.sound || soundDefaults(), p = soundPresentation(view.sound, view.now_ms);
  const encoded = JSON.stringify(config);
  if (!dirty && applied !== encoded) { load(config); applied = encoded; }
  $('sound-enabled').checked = config.enabled; $('sound-status').textContent = p.title;
  $('sound-reason').textContent = `${p.state ? p.state + ' · ' : ''}${p.reason}`;
  $('sound-next').textContent = p.next; $('sound-rounds').textContent = p.rounds;
  $('sound-limit-live').textContent = p.until; $('sound-error').textContent = view.sound?.error || '';
  soundStates.forEach(key => {
    id(key, 'test').disabled = !p.canTest;
    id(key, 'test').title = p.canTest ? '按照这张卡片的模式和音量试听一轮' : '需蜂鸣开启、电量有效、设备连接，且当前无未停止的任务声音';
  });
}
