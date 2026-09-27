# 安装、配置、升级和卸载

目标拓扑：Codex 在 Ubuntu，灯配对到 Windows，Windows 运行 AI Light。预编译 agent 以 Ubuntu 24.04 x86_64 为构建基准；其他发行版或架构请从源码构建。

## 1. Windows 安装

从同一个 Release 下载安装包与 SHA256SUMS。PowerShell 校验：

```powershell
Get-FileHash .\AI-Light-0.3.3-windows-x64-setup.exe -Algorithm SHA256
```

将结果与发行页校验清单比较。安装包未签名，核对来源，不关闭安全防护。系统需要 WebView2 Runtime；安装器配置为缺失时下载其官方引导程序，不是完全离线安装。若提示 VC Runtime DLL 缺失，使用微软官方 Visual C++ 2015–2022 x64 Redistributable，不要到 DLL 下载站拷贝文件。

灯在 Windows 设置中配对后启动 AI Light。如果只有一台兼容 HID 灯，首次启动尝试自动选择；否则在「设备与电池」点击扫描，选择后保存。手填时使用该设备 12 位十六进制地址（不含冒号）。不要使用他人的设备地址。

主页四个灯效测试是独立预览，不伪造 Codex 状态。声音测试需开启总开关、设备在线且有有效非低电量读数。

## 2. VM 网络与连接配置

Windows PowerShell 查看宿主机地址：

```powershell
Get-NetIPAddress -AddressFamily IPv4 | Format-Table InterfaceAlias,IPAddress
```

Ubuntu 查看自己的地址：

```bash
hostname -I
```

桥接/NAT/Host-only 的可达地址不同，不直接把虚拟机网关当作 Windows。Windows 的「Codex 接入」填写 Ubuntu 可以访问的宿主机 IP，导出 `client.json`。默认监听 TCP 17322。

导出文件位于 `%LOCALAPPDATA%\AILight\exports\client.json`。用自己配置好的共享目录或 SSH 传到 Ubuntu，保留私密权限；不要贴到 issue。

Windows 程序内的「仅允许的虚拟机 IP」建议填写真实 VM IP。若防火墙拦截，在管理员 PowerShell 中仅为该来源添加规则（替换占位符）：

```powershell
New-NetFirewallRule -Name 'AILightFromUbuntu' -DisplayName 'AI Light from Ubuntu' -Direction Inbound -Action Allow -Protocol TCP -LocalPort 17322 -RemoteAddress 'VM_IP' -Profile Any
```

程序不会关闭防火墙、创建公网端口映射或自动扩大规则范围。VM IP 变化需调整白名单和防火墙。变更监听 IP/端口后从托盘退出再打开，并重新导出配置。

## 3. 安装 Ubuntu agent

解压发行包，例如：

```bash
mkdir -p ~/ai-light-agent
cd ~/ai-light-agent
tar -xzf ~/Downloads/ai-light-agent-0.3.3-linux-x86_64.tar.gz
chmod 700 .
# 将 Windows 导出的 client.json 放到当前目录
chmod 600 client.json
./light-agent install --client ./client.json
```

**不要为了安装而 sudo 整个 agent。使用运行 Codex 的同一个 Linux 用户。** 自定义 `CODEX_HOME` 时，安装器与 Codex 要使用同一个环境变量。

安装器写入：

- `~/.local/bin/light-agent`。
- `~/.config/ai-light/client.json`（配置与访问密钥）。
- `~/.local/state/ai-light/state.json`（最小状态，不含对话正文）。
- `$CODEX_HOME/hooks.json`，默认 `~/.codex/hooks.json`；先备份，再合并自己的 8 项 command hooks。
- `$CODEX_HOME/config.toml`：仅当根级没有已有 `notify` 时，前置加入指向 `light-agent notify` 的用户级完成回调；已有通知器保持原样，不覆盖。
- `~/.config/systemd/user/ai-light-relay.service`。

不会修改模型、provider、代理、`AGENTS.md` 或权限批准逻辑。XDG 配置/状态目录变量适用于配置与状态。

检查与启用：

```bash
systemctl --user status ai-light-relay.service --no-pager
~/.local/bin/light-agent check
journalctl --user -u ai-light-relay.service -n 60 --no-pager
```

需要无人登录时也运行用户服务：

```bash
sudo loginctl enable-linger "$USER"
systemctl --user enable --now ai-light-relay.service
```

没有 user systemd 时：

```bash
./light-agent install --client ./client.json --no-service
~/.local/bin/light-agent relay
```

## 4. 信任 Codex hooks

重启 Codex，输入 `/hooks`，审阅并信任指向本机 `~/.local/bin/light-agent hook` 的规则。应有 8 个事件：SessionStart、UserPromptSubmit、PreToolUse、PermissionRequest、PostToolUse、Stop、Interrupt、SessionEnd。

完成状态还有第二条本地路径：Codex 用户级 `notify` 会把 `agent-turn-complete` JSON 作为最后一个 argv 参数交给 `light-agent notify`。它不注入上下文、不需要模型调用，也不出现在 `/hooks`。AI Light 只接受已由 hooks 登记的同一 session/turn，因此陌生线程不会点亮绿灯。部分 `codex exec` 版本不触发 Stop/PostToolUse，而 notify 可补上完成状态；见 https://github.com/openai/codex/issues/18607 。

若安装器提示“Existing Codex notify setting left unchanged”，说明你已经配置了别的 notify；AI Light 不会抢占它，此时交互式 Stop 仍有效，但 `codex exec` 的完成兜底不会自动启用。需要同时使用两个通知器时，应由现有通知脚本把收到的同一 JSON payload 再调用一次 `~/.local/bin/light-agent notify "$PAYLOAD"`。

若 `/hooks` 不存在或事件不可用，先记录 `codex --version` 并核对官方文档，不假定任何版本都兼容。旧 Python 灯控规则若还指向已删除脚本，审阅后仅移除那些旧规则，不动其他项目 hooks。

之后通常直接运行 `codex`。额外监测进程异常退出：

```bash
~/.local/bin/light-agent codex --
~/.local/bin/light-agent codex -- resume
```

它继承终端，不重复提交问题，不额外创建模型会话。

## 5. 联调

逐条运行并观察，不要把四条一次性连着执行：

```bash
~/.local/bin/light-agent emit working  # 黄灯呼吸
~/.local/bin/light-agent emit waiting  # 黄灯常亮；允许时开始声音规则
~/.local/bin/light-agent emit error    # 红灯呼吸
~/.local/bin/light-agent emit done     # 绿灯，五分钟到期
~/.local/bin/light-agent emit off      # 清除这个手工测试会话
```

`emit` 只更新本地测试会话，需要 relay 运行才发送。不要遗留 `error` 测试状态，以免其优先级盖住正常任务。`light-agent clear` 会清空本来源全部会话，需明确知道影响后才使用。

## 6. 使用声音设置

声音页面可调每状态模式、音量、延迟、重复间隔、时长或次数。总开关即时生效；规则编辑需要保存。保存声音规则会停止当前轮，不补播旧提醒。

低电量 ≤20% 强制禁鸣，包括试听；连续两次 ≥25% 且间隔至少10秒解除。电量未知/失败/过期也静音。声音总开关仍可显示开启，旁边会说明为什么暂停。音量百分比映射为原始驱动0–50，不是声压的线性百分比。

「停止本次声音」只取消当前状态提醒，新任务仍可提醒。「暂停灯光」只暂停灯的输出，不等于关闭声音或退出程序。彻底停止所有后台请使用托盘退出。

## 7. 升级

Windows 从托盘退出旧实例，再安装同一路径的新安装包。不要删除 `%LOCALAPPDATA%\AILight`，其中保存设备、设置和密钥。旧版已存在序列号会保留；自动扫描只用于没有选择目标的公开版新配置。

Ubuntu 使用同一 Linux 用户运行新二进制的 `install --client ...`，会保留已有来源名并重启服务。**从 0.3.2 升级到 0.3.3 必须重新运行一次 install**，因为本版需要检查并安装新的 Codex notify 完成兜底；单独替换 Windows 程序不能修复“任务完成仍黄灯呼吸”。

## 8. 卸载

Windows：托盘退出后，在 Windows 应用设置卸载 AI Light。安装器移除自己的自启项和程序，默认保留 `%LOCALAPPDATA%\AILight`；要删除密钥/偏好，确认不再使用后手动删除该目录。手动添加的防火墙规则也由你删除：

```powershell
Remove-NetFirewallRule -Name 'AILightFromUbuntu'
```

Ubuntu：

```bash
~/.local/bin/light-agent uninstall
```

仅删除自己的服务、二进制和 hook 命令，并且只在 `config.toml` 顶部仍是安装器写入的那条 AI Light notify 时删除它；用户原有或后来修改的 notify 不碰。保留本地 AI Light 配置与状态。开启的 lingering 是用户级系统设置，卸载不会擅自关闭，因为可能有其他用户服务依赖它。
