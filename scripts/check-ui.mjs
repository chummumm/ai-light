import fs from 'node:fs';
import assert from 'node:assert/strict';
const html=fs.readFileSync('ui/index.html','utf8');
const ids=[...html.matchAll(/\bid="([^"]+)"/g)].map(m=>m[1]);
assert.equal(ids.length,new Set(ids).size,'Duplicate HTML ids');
const required=new Set();
for(const f of ['ui/app.js','ui/sound-ui.js']){
 const src=fs.readFileSync(f,'utf8');
 for(const m of src.matchAll(/(?:\$|text)\('([a-z][a-z0-9-]*)'/g)) required.add(m[1]);
}
for(const id of required) assert.ok(ids.includes(id),`Missing static DOM element ${id}`);
assert.ok(html.includes('id="scan-devices"'));
assert.ok(/DEFAULT_SERIAL:\s*&str\s*=\s*""/.test(fs.readFileSync('crates/light-core/src/protocol.rs','utf8')),'Public builds must not embed a default personal address');
console.log(`UI structure checked: ${ids.length} unique IDs.`);
