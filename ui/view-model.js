export const stateInfo = Object.freeze({
  off: { title: '空闲，等你开始。', effect: '全部熄灭', description: '当前没有需要显示的任务状态。' },
  working: { title: '正在认真工作。', effect: '黄灯 · 柔和呼吸', description: 'Codex 正在执行任务，此刻无需守在屏幕前。' },
  waiting: { title: '这一步，需要你。', effect: '黄灯 · 常亮', description: '有问题或权限申请等待处理，请回到 Codex 查看。' },
  done: { title: '这一轮，完成了。', effect: '绿灯 · 常亮', description: '回合已结束。倒计时到期后自动熄灯。' },
  error: { title: '执行中遇到异常。', effect: '红灯 · 柔和呼吸', description: '请检查 Codex 返回结果；重试或继续执行后会更新状态。' },
});
export function remaining(ms) {
  const seconds = Math.max(0, Math.ceil((Number(ms) || 0) / 1000));
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
}
export function clock(ms) {
  if (ms == null) return '—';
  return new Date(ms).toLocaleTimeString('zh-CN', { hour12: false });
}
export function yesNo(value) { return value === true ? '已连接' : value === false ? '未连接' : '未知'; }
export function batteryPresentation(battery) {
  const data = battery?.reading;
  const valid = Number.isInteger(data?.percent) && data.percent >= 0 && data.percent <= 100;
  const old = Boolean(battery?.stale);
  return { valid, old, percent: valid ? data.percent : null, number: valid ? String(data.percent) : '—',
    summary: valid ? `${data.percent}%` : '—', suffix: old && valid ? '上次读数' : '',
    low: valid && !old && data.percent <= 20,
    note: data?.note || '标准 BLE 2A19 · Uncached，直接读取设备。数值为固件估算。' };
}
export function powerPresentation(power) {
  const value = power?.reading?.voltage_mv;
  const valid = Number.isInteger(value) && value >= 2000 && value <= 5000;
  const old = Boolean(power?.stale) || !power?.enabled;
  return { valid, old, enabled: power?.enabled === true,
    voltage: valid ? `${(value / 1000).toFixed(3)} V${old ? ' · 上次读数' : ''}` : '—',
    voltageShort: valid ? `${(value / 1000).toFixed(3)} V${old ? ' · 上次' : ''}` : '—',
    source: 'HID READ 0x14 · 小端 u16 · mV；与兼容客户端解析一致',
    raw: power?.reading?.raw_14 || '—' };
}
export const soundStates = ['waiting', 'error', 'done', 'working'];
export const soundNames = { waiting: '等待人工处理', error: '执行异常', done: '任务完成', working: '工作中' };
export const modeNames = { short: '短鸣', double: '双响', triple: '三响', long: '长鸣' };
export function soundDefaults() {
  return { enabled: true, default_volume: 70,
    working: { enabled: false, mode: 'short', volume: null, delay_seconds: 0, interval_seconds: 60, limit: { kind: 'once' } },
    waiting: { enabled: true, mode: 'double', volume: null, delay_seconds: 2, interval_seconds: 15, limit: { kind: 'duration', seconds: 180 } },
    error: { enabled: true, mode: 'triple', volume: 80, delay_seconds: 3, interval_seconds: 10, limit: { kind: 'duration', seconds: 120 } },
    done: { enabled: true, mode: 'short', volume: 50, delay_seconds: 0, interval_seconds: 10, limit: { kind: 'duration', seconds: 30 } } };
}
export function rawSoundVolume(percent) {
  if (!Number.isInteger(percent) || percent < 0 || percent > 100) throw new Error('音量必须为 0–100 的整数');
  return Math.ceil(percent / 2);
}
export function validateSoundConfig(config) {
  if (typeof config?.enabled !== 'boolean') throw new Error('缺少蜂鸣总开关');
  rawSoundVolume(config.default_volume);
  for (const key of soundStates) {
    const r = config[key];
    if (typeof r?.enabled !== 'boolean' || !Object.hasOwn(modeNames, r?.mode)) throw new Error('无效声音模式');
    if (r.volume !== null) rawSoundVolume(r.volume);
    if (!Number.isInteger(r.delay_seconds) || r.delay_seconds < 0 || r.delay_seconds > 300) throw new Error('首次延迟必须为 0–300 秒');
    if (!Number.isInteger(r.interval_seconds) || r.interval_seconds < 5 || r.interval_seconds > 3600) throw new Error('重复间隔必须为 5–3600 秒');
    if (r.limit?.kind === 'duration') {
      if (!Number.isInteger(r.limit.seconds) || r.limit.seconds < 5 || r.limit.seconds > 3600) throw new Error('时限必须为 5–3600 秒');
    } else if (r.limit?.kind === 'count') {
      if (!Number.isInteger(r.limit.count) || r.limit.count < 1 || r.limit.count > 1000) throw new Error('次数必须为 1–1000');
    } else if (!['once', 'until_state'].includes(r.limit?.kind)) throw new Error('无效停止条件');
  }
  return config;
}
export function soundPresentation(sound, now = Date.now()) {
  const labels = { disabled: '蜂鸣已关闭', low: '低电量，蜂鸣暂停', recovering: '等待电量恢复确认', unknown: '电量未确认，蜂鸣暂停', offline: '设备离线，声音暂停', preview: '试听一轮', playing: '正在提醒', scheduled: '提醒已安排', quiet: '本次提醒已停止', idle: '已开启，等待任务' };
  const status = sound?.status || 'unknown';
  return { title: labels[status] || '声音状态待确认', reason: sound?.reason || '等待后台状态',
    low: Boolean(sound?.low_battery), ready: sound?.gate === 'ready',
    next: sound?.next_ms != null ? remaining(sound.next_ms - now) : '—',
    until: sound?.until_ms != null && ['scheduled', 'playing', 'preview'].includes(status) ? `${remaining(sound.until_ms - now)} 后结束` : '',
    rounds: `${sound?.sent_rounds || 0} 轮已发送`, canTest: sound?.can_test === true,
    state: soundNames[sound?.active_state] || '',
    guard: sound?.gate === 'ready' ? '电量允许蜂鸣' : sound?.gate === 'low' ? '低电量，已禁鸣' : sound?.gate === 'recovering' ? '等待连续两次 ≥25%' : '电量未确认，暂停蜂鸣' };
}
