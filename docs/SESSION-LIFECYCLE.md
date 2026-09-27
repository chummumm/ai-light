# 0.3.4 会话生命周期修复与无中断升级

## 为什么任务结束后仍会黄灯呼吸

旧版把 Ubuntu relay 的心跳当作来源内所有记录仍有效的依据。只要一条遗留 `working` 没有收到结束事件，它就会压住其他回合的 `done`。记录数量也包括完成、空闲及历史记录，不能解释为同时运行的任务数。

这版保留多任务优先级：异常 > 等待人工 > 工作中 > 完成 > 空闲。**确实还有其他任务在运行时，继续黄灯呼吸是正确行为。** 修复不是提高绿灯优先级，而是让已经结束的旧记录不再冒充运行任务。

## 有任务正在运行时怎么升级

使用运行 Codex 的同一 Linux 用户，在**新发行包解压目录**执行：

```bash
./light-agent upgrade
~/.local/bin/light-agent check
~/.local/bin/light-agent lifecycle-status
```

`upgrade` 先核对现有 AI Light 配置，再将新二进制以可执行权限写入同目录临时文件，fsync 后原子替换 `~/.local/bin/light-agent`，最后只重启 `ai-light-relay.service`。**不调用 Codex，不结束任务，不清空 state.json，不改 hooks.json/config.toml，不覆盖 client.json。** 不需要重新导出 Windows 的连接密钥。

自定义 Codex 目录时，把实际值传给升级命令：

```bash
CODEX_HOME=/your/actual/codex-home ./light-agent upgrade
```

仅将这个目录记入本地私有跟踪文件供 relay 使用，不改 Codex 配置。没有 user-systemd 的环境用 `upgrade --no-service`，再通过现有进程管理器重启 **AI Light relay**，不要重启 Codex。

已有 hook 调用的绝对路径没有变，下一次调用自动使用新二进制。独立观察器可核对升级前已经登记的会话，因此无需为了观察正在执行的任务而重启 Codex。对本轮正在运行的旧 Codex 进程，新增 `notify` 配置可能不会立即重载；该补偿路径不依赖它。

协议 schema 仍为 1，**只升级 Ubuntu agent 就可以修复生命周期；旧 Windows 0.3.2/0.3.3 接收端可以继续运行。** Windows 0.3.4 另外改进会话明细展示，方便时再升级，不必现在退出。

## 三类证据

1. **正常 hooks 与 notify**：保留原有工作、审批、工具、停止和完成通知路径。同一回合的迟到工具输出不重新启动已完成任务；重复提示和通知不重置绿灯期限。`Stop` 可以被其他 hook 要求继续，所以新的工具调用可重新进入工作；已收到明确完成通知/日志终态之后则不接受迟到工具调用。
2. **只读 rollout 补偿**：仅在已登记的 Codex 目录 `sessions` / `archived_sessions` 下按已知会话 ID 匹配文件，并核对首条 `session_meta.payload.id`。识别 `event_msg` 的 `task_started`/`turn_started`、`task_complete`/`turn_complete`、`turn_aborted`，核对 turn_id 和原始 UTC 时间戳。发现匹配终态才修正该回合；明确的 subagent 元数据使相应辅助记录退出聚合。读取按每文件 4 MiB 窗口/每行 1 MiB 上限进行，之后按文件偏移增量跟踪，未写完整的 JSON 行等下一次读取。
3. **进程身份**：从 hook 祖先进程定位实际名为 `codex` 的进程，保存 PID + `/proc/PID/stat` 启动 tick + boot_id。历史会话可关联确实打开该 rollout 的 Codex 进程。只有曾成功关联的身份明确消失、变成僵尸、PID 被复用或系统重启，且经过 10 秒宽限、重新检查后，才退出运行聚合。进程退出不是成功完成，不伪造绿灯。权限不足、读取失败、找不到可关联进程均是未知，不当作死亡。

文件读取发生在状态锁以外，应用结果时再次比较 session_id、turn_id、session version；进程退出结果还复核最新绑定。观察期间新到的 hook 不会被旧查询结果覆盖。观察线程与网络重试线程独立，心跳不更改会话生命周期。

## 旧记录怎么迁移

不清空、不批量假定完成。逐条核对已有记录：有匹配完成证据的，按**原始完成时间**恢复状态；一天前的完成不会从现在重新亮绿灯五分钟。有真实活动或活着的关联进程的，保留。手工 `manual-test` 的工作/等待/异常测试有独立 60 秒期限；真实任务没有“沉默 N 分钟就结束”的期限。完成/空闲记录一小时后回收，即使没有新 hook 也会执行。

## 诊断与边界

`light-agent lifecycle-status` 显示每条记录的核验证据，如 `journal_open`、`journal_terminal`、`owner_alive`、`journal_pending`、`journal_unavailable`、`different_turn`、`unverified`。它不输出 transcript 路径、进程命令行、访问密钥或对话正文。

Windows 显示的是“工作/等待/异常/完成/未参与”分类及逐会话最后事件。`状态持续` 是该状态持续时间，不是最后 token 时间、最后 hook 时间或实际 CPU 活动时间。来源“转发器在线”不承诺每条记录正在运行。

**如果旧记录同时缺少可解析终态和曾成功关联的进程身份，就无法安全推断它是否结束。** 本版保留原状态并报告待核验，不以消灭黄灯为目标编造完成。如果出现这种边界，需要适配相应 Codex 事件格式，而不是清空所有会话。rollout 格式不是稳定的公共接口，已知字段以外的事件不猜测；正常 hooks/notify 仍然保留。

文件可在内存中包含最后回答以计算原有“等待人工”启发式，但不将正文写进跟踪文件、日志或 Windows 快照。`tracking.json` 位于 AI Light 私有状态目录，只有本地需要的路径、进程身份、回合和调用 ID；不要公开这个文件。观察器不会修改 Codex 的会话文件，不访问 Codex 的数据库，不批准操作、不注入上下文、不调用模型。

参考接口：
- https://developers.openai.com/codex/hooks （特别是 Stop 继续执行、SessionEnd 延迟及 transcript 非稳定接口说明）
- https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs
- https://github.com/openai/codex/blob/main/codex-rs/docs/protocol_v1.md


## 0.3.5 补充：无证据旧记录

旧版“未核实仍参与灯态”的规则在仅有旧 UserPromptSubmit 且从未绑定的迁移记录上不再适用。见 [未核实旧记录](UNVERIFIED-LEGACY.md)：单列未知、保留原始状态、允许恢复；绝不把无日志推断为完成。
