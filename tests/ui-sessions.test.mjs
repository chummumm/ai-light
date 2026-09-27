import { test } from 'node:test';
import assert from 'node:assert/strict';
import { counts, summary, caption, detail } from '../ui/sessions-ui.js';
test('records are not claimed to be running', () => { const rows = [...Array(7).fill({state:'off'}), {state:'done'}];assert.equal(counts(rows).working,0);assert.match(summary(rows),/完成 1/);assert.match(caption(rows),/8 条状态记录/);assert.match(caption(rows),/不等于/); });
test('true remaining work retains its own count',()=>assert.equal(counts([{state:'working'},{state:'done'}]).working,1));
test('unknown states do not become working',()=>assert.equal(counts([{state:'future'}]).unknown,1));
test('state duration is not labeled last activity',()=>{const t=detail({state:'working',entered_ms:100,event:'PreToolUse'},1100);assert.match(t,/状态持续 1 秒/);assert.doesNotMatch(t,/最后活动/);});
test('negative duration clamps',()=>assert.match(detail({state:'done',entered_ms:200},100),/0 秒/));
test('missing timestamp stays unknown',()=>assert.match(detail({state:'off'},100),/未知/));
