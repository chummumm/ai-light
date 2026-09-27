# AI Light

**专注工作，抬眼知进度。**

[![Build and release](https://github.com/chummumm/ai-light/actions/workflows/build.yml/badge.svg)](https://github.com/chummumm/ai-light/actions/workflows/build.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

Rust + Tauri 2 的 Windows 桌面灯控，配套全 Rust Ubuntu Codex 状态转发器。把工作、等待、完成和异常变成实体灯光与可配置的声音提醒。**不安装卖家软件，不需要 Python，不调用额外模型。**

[下载安装包](https://github.com/chummumm/ai-light/releases) · [从零配置](docs/GETTING-STARTED.md) · [源码编译](BUILD.md) · [云构建与发布](docs/RELEASING.md) · [协议说明](docs/PROTOCOL.md) · [常见问题](docs/TROUBLESHOOTING.md)

> 当前为早期预发布软件。云端测试和编译不等于 Windows 托盘、真实蓝牙和所有固件已完成验收。安装包未做商业代码签名。请核对发行页对应提交及 SHA256SUMS；不要关闭系统安全防护。

## 支持什么设备

针对兼容 **YY-AiLight / PromLight 风格 `5E 5E` BLE HID 协议**的设备，严格匹配 VID `07D7`、PID `6B01`、Usage Page `FF00`、Usage `0001`。不是通用蓝牙 RGB 控制器。蜂鸣、电量和电压还取决于具体固件能力。

首次启动只在恰好发现一台兼容设备时自动选择；多台或未发现时，到「设备与电池」扫描并选择。**公共源码没有预填任何用户的蓝牙地址、访问密钥或局域网配置。**

```text
Ubuntu VM                                  Windows 实体机
Codex → Rust hook / notify → 本地状态 → Rust relay ──HTTP──→ AI Light
                                                   ├─ 图形界面 / 系统托盘
                                                   ├─ 状态聚合 / 倒计时 / 声音调度
                                                   └─ BLE HID → 实体灯
```

蓝牙留在 Windows，不需要虚拟机直通。网络状态在 Ubuntu 本地写入，独立 relay 负责重试；不会让 hook 等待蓝牙。

## 灯光与声音

| 状态 | 灯光 | 默认声音 |
|---|---|---|
| 工作中 | 黄灯柔和呼吸，3 秒周期 | 不响，可配置 |
| 等待人工处理 | 黄灯常亮 | 双响，延迟 2 秒，每 15 秒一轮，最多 3 分钟 |
| 执行异常 | 红灯柔和呼吸 | 三响，延迟 3 秒，每 10 秒一轮，最多 2 分钟 |
| 回合完成 | 绿灯常亮，5 分钟后熄灭 | 短鸣，每 10 秒一轮，最多 30 秒 |
| 空闲 | 熄灭 | 不响 |

声音有总开关、每状态开关、模式、音量、首次延迟、重复间隔，以及一次/限时/限次/直到状态结束的停止条件。托盘和主页可停止本次声音，不关闭下次提醒。重复事件不重启计时，旧任务倒计时不会熄灭新任务的灯。

电量读取标准 `2A19`；电压读取厂商 `0x14`。**不显示或推断充电状态。**

- 电量 ≤20%：停止并禁用蜂鸣和试听，灯光照常工作。
- 连续两次 ≥25%，间隔至少 10 秒：解除低电量保护，不补播旧声音。
- 未知、失败或过期电量：不启动蜂鸣。
- UI 音量 100% 对应原始驱动 50，避免此固件原始值 100 反而无声；这是相对档位，不是线性声压百分比。

## 日常操作

单击托盘打开窗口；右上角 × 只隐藏窗口。托盘「退出 AI Light」停止接收及后台，并尽力停止声音/熄灯。首次初始化默认启用当前用户登录自启，自启不弹窗口，可在偏好设置关闭。安装器创建开始菜单入口；Windows 可能把托盘图标放进折叠区，需要手动拖出。

深色/浅色/跟随系统主题，七个本地页面：总览、灯效、声音、设备电池、Codex 接入、事件、偏好。界面无外部字体或 CDN。直接打开 HTML 只是未连接的静态界面，不会假装连接了设备。

## 最短安装流程

### Windows

1. 在 Windows 蓝牙设置中配对并打开兼容灯。
2. 从 [Releases](https://github.com/chummumm/ai-light/releases) 下载 `AI-Light-<版本>-windows-x64-setup.exe`，核对校验后安装。
3. 在「设备与电池」确认目标；在主页测试灯光，等待有效电量后再试听。
4. 在「Codex 接入」填写 Ubuntu 能访问到的 Windows 宿主机 IP，导出 `client.json`。这个文件包含访问密钥，不要公开。

### Ubuntu

下载同一发行版的 `ai-light-agent-<版本>-linux-x86_64.tar.gz`，解压到普通用户目录，将 Windows 导出的 `client.json` 放进去，然后：

```bash
./light-agent install --client ./client.json
~/.local/bin/light-agent check
systemctl --user status ai-light-relay.service --no-pager
```

必须使用运行 Codex 的同一个 Linux 用户。安装器会合并 8 个 command hooks；若用户级 `$CODEX_HOME/config.toml` 没有已有 `notify`，还会加入 AI Light 的 `agent-turn-complete` 完成兜底。已有 `notify` 永远不覆盖。重启 Codex，打开 **`/hooks`**，审阅并信任 8 个 hooks；`notify` 是 Codex 自己的用户级完成回调，不出现在 `/hooks` 信任列表中。

测试时逐条运行、观察实体灯：

```bash
~/.local/bin/light-agent emit working
~/.local/bin/light-agent emit waiting
~/.local/bin/light-agent emit done
~/.local/bin/light-agent emit off
```

网络、防火墙、自启、升级及卸载步骤见 [完整使用说明](docs/GETTING-STARTED.md)。

## 构建与开源

云端采用 Windows 原生 MSVC 构建安装包、Ubuntu 24.04 构建 agent，全部使用同一提交和 `Cargo.lock`。普通提交生成 Actions artifacts；版本标签、手动发布或带 `[release]` 的 main 提交可在所有作业成功后发布预发布版。

```bash
# Ubuntu 只编译 agent
bash scripts/build-ubuntu.sh

# Ubuntu 同时交叉编译 Windows 安装包；需要 Microsoft SDK/CRT 下载许可
bash scripts/build-all-ubuntu.sh --install-deps
```

常规源码构建不要求 Node/npm：静态界面已在仓库，图标由 Rust build script 生成。CI 使用 Node 运行前端测试、收集许可证并安装 Tauri CLI，不是应用运行依赖。详细 Windows 原生构建见 [BUILD.md](BUILD.md)。

## 状态识别边界

交互式 Codex 正常以 `Stop` 表示回合结束；AI Light 0.3.3 另外使用 Codex 用户级 `notify` 的 `agent-turn-complete` 作为完成兜底，因为部分 `codex exec` 版本只触发 `UserPromptSubmit` 而不触发 `Stop/PostToolUse`。兜底只接受已经由 hook 登记过的同一 session/turn，不会凭一个陌生通知创建“完成”状态。上游参考：https://github.com/openai/codex/issues/18607

`Stop` / `agent-turn-complete` 都只表示回合结束，不保证任务结果正确。普通文字提问是本地启发式，可能误判；审批等待可能到工具返回时才解除。CLI 没有退出的部分 API/网络错误没有完整通用 hook 覆盖。可用 `light-agent codex -- ...` 额外监控整个 CLI 进程异常退出。

灯控 hook 不调用模型、不添加上下文、不替用户批准，也不阻止 Stop 要求继续推理，所以不会自行增加模型 token 请求。存在一次本地 Rust 进程和状态文件操作的开销。完整限制见 [架构与安全边界](docs/ARCHITECTURE.md)。

## 贡献与许可

原创代码及图标使用 [MIT](LICENSE)。依赖保留各自许可证；发行包提供依赖清单和许可证。协议事实参考 [promlight-linux](https://github.com/DiGuStudent/promlight-linux) 及兼容设备的公开手册，并经过用户设备测试逐步核对。仓库不分发卖家程序、固件、私人日志或字体。项目与 OpenAI、设备厂商、Microsoft 没有隶属或背书关系。

欢迎按 [CONTRIBUTING.md](CONTRIBUTING.md) 提交问题和 PR；不要在 issue 中上传 `client.json`、`secrets.json` 或完整未脱敏诊断。
