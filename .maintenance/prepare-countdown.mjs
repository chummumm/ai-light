// One-time, branch-only preparation. Removed before the production PR.
import fs from 'node:fs';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
const read=p=>fs.readFileSync(p,'utf8');
const write=(p,s)=>{fs.mkdirSync(p.slice(0,p.lastIndexOf('/'))||'.',{recursive:true});fs.writeFileSync(p,s);};
function edit(p,from,to){const s=read(p);if(s.split(from).length!==2)throw Error(`ambiguous/missing anchor: ${p}`);write(p,s.replace(from,to));}
function hash(p){const b=fs.readFileSync(p);return crypto.createHash('sha1').update(`blob ${b.length}\0`).update(b).digest('hex');}
const originals={
 'crates/light-core/src/model.rs':'610571777759f8fa1f180ef21248eeb510559302',
 'src-tauri/src/main.rs':'86d5ac443b82a743d3ed897092a5105f4db63425',
 'src-tauri/src/server.rs':'481f2bface230de30ae8a9d75465a0023bef10f5',
 'scripts/release.sh':'056af2f3370a437b5fa3619a4eff45e3af96242a',
 '.github/workflows/build.yml':'30258aa2f5f93c6fc3c9dc573632a3feffde120c'
};
for(const [p,h] of Object.entries(originals))if(hash(p)!==h)throw Error(`source changed: ${p}`);
if(process.argv.includes('--reproduce')){
 const p='crates/light-core/tests/countdown_baseline.rs';
 write(p,`use light_core::model::{Engine,LightState,Snapshot,WireSession};
#[test]fn completion_must_survive_default_source_timeout(){
 let mut e=Engine::default();
 e.accept(Snapshot{schema:1,source_id:"vm".into(),generation:"g".into(),revision:1,sessions:vec![WireSession{id:"s".into(),turn:"t".into(),version:1,state:LightState::Done,age_ms:0,event:"Stop".into()}]},1_800_000_000_000,300).unwrap();
 assert_eq!(e.view(1_800_000_090_001,90).state,LightState::Done);
}
`);
 const r=spawnSync('cargo',['test','-p','light-core','--locked','--test','countdown_baseline'],{encoding:'utf8'});
 const log=(r.stdout||'')+(r.stderr||'');fs.writeFileSync('baseline-countdown.log',log);fs.unlinkSync(p);
 if(r.status===0||!log.includes('completion_must_survive_default_source_timeout')||!log.includes('left: Off')||!log.includes('right: Done'))throw Error('baseline did not reproduce expected assertion failure');
 console.log('0.3.5 countdown defect reproduced: actual Off, expected Done at 90.001 seconds.');process.exit(0);
}
if(!process.argv.includes('--apply'))throw Error('expected --reproduce or --apply');
edit('crates/light-core/src/model.rs',
 '                let state = if online && !expired { session.wire.state } else { LightState::Off };',
 `                // A received completion has a local deadline independent of relay liveness.
                // Preserve the real online flag: offline sources must not re-enable sound.
                let retain_completion = session.wire.state == LightState::Done;
                let state = if !expired && (online || retain_completion) { session.wire.state } else { LightState::Off };`);
edit('crates/light-core/src/model.rs',
 '    pub fn forget_source(&mut self, id: &str) { self.sources.remove(id); }',
 `    /// Adjust pending completion deadlines from the original completion time.
    /// Changing settings must not revive expired reminders or reset sound episodes.
    pub fn reconfigure_done_timer(&mut self, now: u64, done_seconds: u32) -> Result<()> {
        ensure!((1..=3600).contains(&done_seconds), "invalid completion timer");
        let duration_ms = u64::from(done_seconds) * 1000;
        for source in self.sources.values_mut() {
            for session in source.sessions.values_mut() {
                if session.wire.state == LightState::Done
                    && session.expires_ms.is_some_and(|deadline| now < deadline)
                {
                    session.expires_ms = Some(session.entered_ms.saturating_add(duration_ms));
                }
            }
        }
        Ok(())
    }
    pub fn forget_source(&mut self, id: &str) { self.sources.remove(id); }`);
edit('crates/light-core/src/model.rs','e.view(300_999,400)','e.view(300_999,90)');
edit('crates/light-core/src/model.rs','e.view(301_000,400)','e.view(301_000,90)');
edit('src-tauri/src/main.rs','    d.config=settings;',
 `    if d.config.done_seconds!=settings.done_seconds {
        let now=shared.now();
        d.engine.reconfigure_done_timer(now,settings.done_seconds)?;
        d.log(now,"settings",format!("完成计时改为 {} 秒；未到期计时按原完成时间调整，已到期提醒不重播",settings.done_seconds));
    }
    d.config=settings;`);
edit('src-tauri/src/server.rs',
 '        "sources":v.aggregate.sources,"hardware":v.hardware,"battery":v.battery})))',
 `        "sources":v.aggregate.sources,"hardware":v.hardware,"battery":v.battery,
        "timing":{"done_seconds":v.config.done_seconds,"source_timeout_seconds":v.config.source_timeout_seconds,
            "remaining_ms":v.aggregate.remaining_ms}})))`);
edit('Cargo.toml','version = "0.3.5"','version = "0.3.6"');
for(const n of ['ai-light','light-agent','light-core'])edit('Cargo.lock',`name = "${n}"\nversion = "0.3.5"`,`name = "${n}"\nversion = "0.3.6"`);
edit('src-tauri/tauri.conf.json','"version": "0.3.5"','"version": "0.3.6"');
edit('ui/index.html','id="version">v0.3.5<','id="version">v0.3.6<');
edit('.github/workflows/build.yml',
 '      - name: Publish verified build assets as a pre-release',
 '      - name: Publish verified build assets');
edit('.github/workflows/build.yml',
 '          RELEASE_VERSION: ${{ needs.prepare.outputs.version }}',
 '          RELEASE_VERSION: ${{ needs.prepare.outputs.version }}\n          RELEASE_STABLE: ${{ contains(github.event.head_commit.message, \'[stable]\') }}');
edit('scripts/release.sh','tag="v$RELEASE_VERSION"',
 `stable="${'${RELEASE_STABLE:-false}'}"
[[ "$stable" == true || "$stable" == false ]] || { echo 'Invalid release channel' >&2; exit 1; }
channel=pre-release
[[ "$stable" != true ]] || channel=stable
tag="v$RELEASE_VERSION"`);
edit('scripts/release.sh','# AI Light $RELEASE_VERSION (pre-release)','# AI Light $RELEASE_VERSION ($channel)');
edit('scripts/release.sh','**Unsigned pre-release.**','**Unsigned build.**');
edit('scripts/release.sh','# Never overwrite an already published release or unrelated tag.',
 `if [[ -f "docs/releases/v$RELEASE_VERSION.md" ]]; then
  printf '\\n' >> "$release/RELEASE-NOTES.md"
  cat "docs/releases/v$RELEASE_VERSION.md" >> "$release/RELEASE-NOTES.md"
fi
# Never overwrite an already published release or unrelated tag.
if git show-ref --verify --quiet "refs/tags/$tag"; then
  [[ "$(git rev-parse "$tag^{commit}")" == "$SOURCE_SHA" ]] || { echo 'Existing tag points at another source commit' >&2; exit 1; }
fi`);
edit('scripts/release.sh','gh release edit "$tag" --draft=false --prerelease',
 `if [[ "$stable" == true ]]; then
  gh release edit "$tag" --draft=false --prerelease=false --latest
else
  gh release edit "$tag" --draft=false --prerelease
fi`);
edit('CHANGELOG.md','# Changelog\n',`# Changelog

## 0.3.6 — Completion countdown repair

- A received completion now retains its original local deadline when the source heartbeat times out. Working/waiting/error liveness protections are unchanged.
- Saving a new completion duration updates pending countdowns from the original completion time; expired reminders are not revived.
- Duplicate messages do not extend timers, and offline cached completions do not enable sound replay.
- The authenticated status API now includes configured duration, source timeout, and remaining completion time for diagnosis.
- No firmware, BLE protocol, Ubuntu hooks or task state is changed. Upgrade Windows to receive the fix; Ubuntu 0.3.5 remains compatible.
- CI includes deterministic countdown regressions. Stable publishing remains gated on both platform builds and explicit release intent.
`);
write('docs/releases/v0.3.6.md',`## 0.3.6 倒计时修复

本次修复在 Windows 接收端。升级 Windows 安装包；Ubuntu 已是 0.3.5 的用户不用重装 agent、hooks 或清空状态。

- 已确认完成的任务按本地倒计时保留，不再被默认 90 秒的来源失联超时提前熄灯。来源仍如实显示失联。
- 保存新的完成时长时，仅调整未到期计时，起点仍是原始完成时间；已过期提醒不重播。
- 重复心跳不延长倒计时；离线完成记录不触发蜂鸣补播；新工作仍能接管灯态。
- 状态 API 增加 timing.done_seconds、timing.source_timeout_seconds 和 timing.remaining_ms，便于区分软件计时与实体灯异常。

设置五分钟表示从原始完成时刻计时，不是收到历史完成消息后重新亮五分钟。手动暂停、退出、清空来源或新任务接管仍会改变输出。

这是对两个已在代码中确认的问题的修复；用户的约一分钟熄灯尚无现场状态可唯一归因。若 output 仍为 done 而实体灯灭了，应继续排查设备或传输，不应把本补丁当作硬件问题已经解决。安装包未做 Authenticode 签名。
`);
write('tests/countdown-wiring.test.mjs',`import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
test('saved duration is applied to the engine before replacing settings',()=>{
 const s=fs.readFileSync('src-tauri/src/main.rs','utf8');
 assert.match(s,/d\\.config\\.done_seconds!=settings\\.done_seconds/);
 assert.ok(s.indexOf('d.engine.reconfigure_done_timer(now,settings.done_seconds)?')<s.indexOf('d.config=settings;'));
});
test('status exposes timing but never credentials',()=>{
 const s=fs.readFileSync('src-tauri/src/server.rs','utf8');
 const start=s.indexOf('async fn status('),end=s.indexOf('async fn sync(');
 const body=s.slice(start,end);
 assert.match(body,/"timing"/);assert.match(body,/"remaining_ms"/);
 assert.doesNotMatch(body,/"token"|"secrets"/);
});
`);
console.log('Prepared 0.3.6; application changes are confined to countdown state, settings wiring, and timing diagnostics.');
