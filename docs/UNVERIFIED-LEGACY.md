# 0.3.5：未核实旧记录不等于正在工作

## 适用问题

0.3.4 会一直保留只有旧 UserPromptSubmit、没有 tracking、没有匹配日志的 working。Windows 因此可能长期黄灯。没有日志不证明任务已完成，也不能证明它是标题生成器。

## 新规则

观察器启动时登记迁移候选；只针对缺少 tracking 的旧 Working/UserPromptSubmit、没有 run_id 的记录。在所有已登记根目录完成无错误搜索、没有日志路径、也没有关联进程时，把候选单列为 legacy_unverified。权限错误、搜索尚未结束、根目录不可用、已有绑定或日志时不执行此降级。没有按“几分钟没活动”结束真实任务的超时规则。

原 state/event/turn/changed_ms/touched_ms 保留，仅增加 unverified_legacy 投影标志；Windows 接收 state=off 和 UnverifiedLegacy:原事件。它不是 Done、不是取消、不触发完成蜂鸣。用户界面显示“未核实”，而不是把它混进工作计数。由于无法从丢失的证据恢复真相，绿灯只代表参与追踪的任务已完成，不代表这些旧记录也被证明完成。

后续收到该会话有效的新 hook，或找到匹配回合生命周期记录，会自动恢复。状态锁内重查版本、回合及 tracking，避免扫描期间覆盖真实任务的新事件。已有绑定的长时间推理不会降级。只识别 thread_title 为内部任务，不把所有子代理一概删除；现有主任务 hooks 仍按父会话聚合，不承诺额外发现全部未登记子代理。

## 升级

使用运行 Codex 的同一用户：解压本版 Ubuntu 包，运行 ./light-agent upgrade。只重启 AI Light relay，不清空 state，不改 Codex 配置或 hooks，不终止 Codex；已有任务继续运行。

Windows 升级到本版可看到明确的“未核实”分类。旧 Windows 0.3.2—0.3.4 会把它显示成不参与或空闲，但不会再被这条旧 working 卡住。

检查：~/.local/bin/light-agent lifecycle-status。legacy_unverified 的 reason 应为 pre_observer_start_without_tracking_or_journal。没有找到身份就不会称其为“子代理”或“已完成”。
