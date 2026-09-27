// Record count is not task liveness. All values are rendered as text, never HTML.
export const isUnverified = s => typeof s?.event === 'string' && s.event.startsWith('UnverifiedLegacy:');
export function counts(sessions = []) {
  const c = { working: 0, waiting: 0, error: 0, done: 0, off: 0, unknown: 0 };
  for (const s of sessions) {
    if (isUnverified(s)) c.unknown++;
    else if (Object.hasOwn(c, s.state)) c[s.state]++;
    else c.unknown++;
  }
  return c;
}
export function summary(sessions = []) {
  const c = counts(sessions);
  return `工作 ${c.working} · 等待 ${c.waiting} · 异常 ${c.error} · 完成 ${c.done} · 未参与 ${c.off} · 未核实 ${c.unknown}`;
}
export function caption(sessions = []) {
  const n = counts(sessions).unknown;
  return n ? sessions.length + ' 条记录；另有 ' + n + ' 条未核实，灯光仅代表已纳入任务' : sessions.length + ' 条状态记录；记录数不等于运行任务数';
}
export function detail(s, now) {
  if (isUnverified(s)) return '未核实历史记录 · 原事件 ' + s.event.slice('UnverifiedLegacy:'.length) + ' · 不计入当前灯态；未认定完成';
  const names = { working: '工作中', waiting: '等待处理', error: '异常', done: '已完成', off: '不参与灯态' };
  const seconds = Number.isFinite(s.entered_ms) ? Math.max(0, Math.floor((now - s.entered_ms) / 1000)) : null;
  return `${names[s.state] || '待核验'} · ${s.event || '未知事件'} · 状态持续 ${seconds === null ? '未知' : seconds + ' 秒'}`;
}
export function renderSources(v, node) {
  const open = new Set(Array.from(node.querySelectorAll('details')).filter(d => d.open).map(d => d.dataset.source));
  node.replaceChildren();
  for (const source of v.aggregate.sources) {
    const records = v.aggregate.sessions.filter(s => s.source === source.id);
    const row = document.createElement('div'); row.className = 'detail-row';
    const left = document.createElement('span'); left.textContent = source.id;
    const right = document.createElement('span'); right.textContent = `${source.online ? '转发器在线' : '转发器失联'} · ${summary(records)}`;
    row.append(left, right); node.append(row);
    const details = document.createElement('details'); details.dataset.source = source.id; details.open = open.has(source.id);
    const title = document.createElement('summary'); title.textContent = `查看 ${records.length} 条记录及最后事件`;
    details.append(title);
    for (const s of records) {
      const line = document.createElement('div'); line.className = 'detail-row';
      const id = document.createElement('span'); id.textContent = s.id === 'manual-test' ? '手工测试（自动到期）' : `${s.id.slice(0, 12)}…`;
      id.title = `${s.id} / ${s.turn || '未知回合'}`;
      const state = document.createElement('span'); state.textContent = detail(s, v.now_ms);
      line.append(id, state); details.append(line);
    }
    node.append(details);
    const unknown = counts(records).unknown;
    if (unknown) {
      const warning = document.createElement('p'); warning.className = 'empty';
      warning.textContent = '存在未核实历史记录：不计入当前灯态，但不代表已完成。新 hook 或匹配的回合日志会恢复追踪。';
      node.append(warning);
    }
  }
  if (!node.children.length) {
    const empty = document.createElement('div'); empty.className = 'empty'; empty.textContent = '尚无来源；转发器在线不代表每条会话都在工作。'; node.append(empty);
  }
}
