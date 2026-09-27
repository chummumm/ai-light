import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
test('saved duration is applied to the engine before replacing settings',()=>{
 const s=fs.readFileSync('src-tauri/src/main.rs','utf8');
 assert.match(s,/d\.config\.done_seconds!=settings\.done_seconds/);
 assert.ok(s.indexOf('d.engine.reconfigure_done_timer(now,settings.done_seconds)?')<s.indexOf('d.config=settings;'));
});
test('status exposes timing but never credentials',()=>{
 const s=fs.readFileSync('src-tauri/src/server.rs','utf8');
 const start=s.indexOf('async fn status('),end=s.indexOf('async fn sync(');
 const body=s.slice(start,end);
 assert.match(body,/"timing"/);assert.match(body,/"remaining_ms"/);
 assert.doesNotMatch(body,/"token"|"secrets"/);
});
