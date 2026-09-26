# 架构与实现边界

## 模块

- `crates/light-core`：HID帧/校验、标准电量解析、会话状态、Codex事件归约、低电量保护及声音调度，无网络和系统调用。
- `crates/light-agent`：Linux CLI、本地JSON状态文件、文件锁、HTTP转发、hook/systemd安装。不是数据库服务。
- `src-tauri`：Windows UI、托盘、单实例、HKCU登录自启、HTTP接收、隔离的HID/GATT子进程。
- `ui`：本地HTML/CSS/ES modules；不在浏览器内直接连接蓝牙。

## 状态同步

Hook通过stdin接收Codex事件JSON，仅在内存检查原始内容，持久化事件名/来源/会话/回合/工具标识与状态；不持久化提示词、回答、命令正文、输出、cwd。hook失败也返回空对象并退出0，避免灯控阻断主任务。所有hook都需要用户按Codex信任机制确认。

Relay每200ms检查本地revision变化，15秒心跳，使用`POST /v1/sync`发送最新完整Snapshot。不是逐条重播过时事件。Bearer token鉴权、请求体上限256KiB、最多32来源/每来源128会话；默认只允许本机/私有网络地址，配置白名单后限制具体来源。`GET /v1/status`同样要求鉴权。

网络请求禁用系统代理和重定向，默认2秒连接/4秒总超时；失败保留最新本地状态重试。HTTP没有传输加密，只用于可信宿主机/虚拟机网络；跨不可信网络须在外面使用TLS或加密隧道，不能仅靠Bearer token。

## 计时与声音

Windows聚合状态优先级：error > waiting > working > done > off。完成期限按会话保留，迟到事件扣除age，重复事件不延长。来源超过90秒无心跳暂不参与灯态；不当成AI任务失败。窗口隐藏不影响后台时钟。

声音按稳定的会话状态episode去重，单一调度器串行输出，不为每个hook创建计时器。高优先级提醒接管后不补播较低优先级队列。启动前的状态、明显迟到的事件、低电量时期取消的声音、网络恢复后的旧声音都不补播。

每次声音片段最多保留5秒，然后发送停止命令；单次模式具体音长由固件决定。音量raw=ceil(UI/2)，仅允许0–50；读回验证后才鸣叫。音量0不发送声音。低电量≤20停止，恢复阈值≥25两次间隔10秒；有效读数最长90秒，失败也暂停声音。此保护不是独立硬件电池保护器。

## Windows生命周期

主程序通过Tauri单实例插件恢复现有窗口。X隐藏，托盘退出设置stop，取消当前声音，给停止/熄灯有限清理窗口。同步蓝牙操作运行在同一exe的`--hardware-worker`子模式，通过管道输入结构化请求，Windows Job Object约束其生命周期。硬件子进程没有独立自启、HTTP服务或看门狗。

灯光/HID READ/声音由同一调度线程串行调用，GATT电量在独立受限子进程读取。断线、驱动堵塞时超时终止该次查询，不让GUI无限等待。输出API成功与实际厂商ACK在UI区分；未取得ACK不能宣称物理LED或声音一定成功。

## Codex覆盖限制

安装8个事件：SessionStart、UserPromptSubmit、PreToolUse、PermissionRequest、PostToolUse、Stop、Interrupt、SessionEnd。具体字段由实际CLI版本决定。该包不要求模型调用灯控工具，不注入additionalContext，不改变审批结果，不以Stop block要求继续推理。

普通文字提问仅启发式；可以在Ubuntu client.json将question_heuristic设false后重启relay。审批通过的精确瞬间不一定有hook，本实现通常在PostToolUse解除等待。子代理带agent_id时过滤，但不同版本字段差异须验收。CLI仍留在终端时的某些模型/API错误未完整覆盖。可选`light-agent codex -- ...`只补充整个CLI进程退出码监测。

## 未承诺的功能

无充电/放电状态推测，无OTA/固件升级，无通用蜂鸣频率/音长合成，无自动下载执行更新，无多个厂商协议自动猜测。已报告的百分比是固件估算，不是校准SOC；电压值的协议单位已核对，ADC物理精度未测量。
