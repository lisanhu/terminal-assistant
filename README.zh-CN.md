# termassist

[English](README.md) | **中文**

分屏终端助手：一边是你日常的 shell，另一边是你选择的嵌套 CLI agent
（[`kimi`](https://github.com/MoonshotAI/kimi-cli)、`claude`、`codex`……
首次运行时选择）——就在你正在用的终端里。不需要 tmux，也不用换终端模拟器。

```
┌ shell: ~/project ────────────┬ agent: kimi — ~/project ────────┐
│ you@host:~/project$ ls       │                                 │
│ ...                          │   Kimi Code                     │
│ you@host:~/project$          │   > _                           │
└──────────────────────────────┴─────────────────────────────────┘
```

两侧都是跑在同一个物理终端里的全交互程序。agent 能看到你这一侧：TUI
对外暴露一个本地 socket，内置的 skill 会教 agent 运行
`termassist read-pane` 来读取你 pane 的屏幕内容和回滚历史——所以你可以
直接问"刚才那条命令为什么报错"，不用复制粘贴任何东西。

## 演示

![termassist 演示：全部快捷键](assets/demo.gif)

全部快捷键演示：焦点切换、布局切换、分隔线移动、回滚模式、
agent 面板隐藏/恢复、退出。（源文件：`assets/demo.tape`，用
[VHS](https://github.com/charmbracelet/vhs) 录制。）

## 安装

```sh
cargo build --release
# 二进制在 target/release/termassist
```

想要短别名 `ta`，在 shell 配置里加一行（`~/.bashrc` / `~/.zshrc`）：

```sh
alias ta=termassist
```

## 键位

除 TUI 自身的键位外，所有输入都转发给当前焦点的 pane。以下键位均可
配置（见[配置](#配置)节），表中为默认值：

| 键 | 作用 |
| --- | --- |
| `Ctrl+G` | 在两个 pane 间切换焦点 |
| `Ctrl+T` | 切换分屏方向（左右 ↔ 上下） |
| `Ctrl+S` | 进入焦点 pane 的回滚模式 |
| `Ctrl+←` / `Ctrl+→` | 移动分隔线（调整分屏比例） |
| `Ctrl+N` | 开关 agent 面板（启动 / 隐藏 / 恢复） |
| `Ctrl+Q` | 退出 |

**回滚模式：** `↑`/`k`、`↓`/`j`、`PageUp`/`PageDown`、`Home`/`g`（顶部）、
`End`/`G`（底部）、`Esc`/`q`（退出）。滚到底部也会自动退出。

**鼠标：** 点击 pane 聚焦，拖拽分隔线调整比例，滚轮翻看历史。焦点 pane
的边框会高亮。调整终端窗口大小会重新布局并 resize 两个 PTY。

### agent 面板状态

| agent 状态 | `Ctrl+N` 的效果 |
| --- | --- |
| 从未启动 / 进程已退出 | 启动它（使用原始启动目录）并显示 |
| 运行中且可见 | 隐藏；shell 独占全屏 |
| 运行中但已隐藏 | 恢复显示 |

## 特性

- **一个窗口，两个真终端**——左边是你的 `$SHELL`，右边是任意 CLI agent
  （首次运行时选择，之后可在配置中更改）。每侧都是真实 PTY，经完整的 vt100 终端模拟器
  渲染，颜色、全屏 TUI、中文/emoji 宽字符都能正确显示。
- **agent 看得见你的终端**——通过本地 socket 上的
  `termassist read-pane` 实现，封装成一份可移植的
  [skill](skills/termassist/SKILL.md)，适用于所有基于 skill 的 agent
  （kimi、Pi、Claude Code……），不依赖 MCP。
- **可开关的 agent 面板**——`Ctrl+N` 启动 / 隐藏 / 恢复。隐藏是
  *挂起，不是杀掉*：agent 在后台继续运行，同时你的 shell 独占全屏。
- **合理的 pane 生命周期**——pane 内进程退出即关闭该 pane，存活的一侧
  自动占满全屏。两个 pane 都以你启动 `termassist` 时的目录作为工作目录。
- **嵌套调用自动转发**——在 pane 里直接敲 `termassist`（或 `ta`）不会
  再嵌套一个新 TUI，而是转发给正在运行的实例：打开或聚焦 agent 面板，
  然后退出。
- **键盘 + 鼠标**——焦点切换、分屏方向切换、拖拽边框调比例、每侧独立
  回滚；所有键位都可配置。
- **架构上跨平台**——平台相关代码只存在于 `src/pty.rs`（portable-pty：
  Unix PTY / Windows ConPTY）和 `src/ipc.rs`（interprocess：Unix socket /
  Windows named pipe）。已在 Linux 上构建和测试；macOS/Windows 可以编译
  但尚未实测。

## 用法

### 首次运行

第一次启动（配置文件还不存在）时，termassist 会在进入 TUI 之前询问
agent pane 里运行的 agent 启动命令（可带参数）。输入有效命令后，会把
它原样写入完整的默认配置文件，回显该命令和配置文件路径
（`agent = "..."` / `config written to <path>`），并提示重新运行
`termassist` 后以状态码 0 退出——向导本身不会进入 TUI；下次启动找到
配置文件便直接进入 TUI。输入为空、全空白或输入流提前结束（EOF）时，
打印"未配置 agent"的提示（包括稍后可以直接编辑的配置文件路径），以
非零状态码退出且不写配置文件，下次启动仍会触发向导。非交互场景
（stdin/stdout 是管道、录屏工具等）完全跳过向导，不写配置文件。之后
随时可以编辑配置文件里的 `agent` 字段更换 agent。

```sh
termassist                 # 首次运行：向导询问 agent 命令、写配置后退出；
                           # 再跑一次进入分屏 TUI（左：$SHELL，右：你配置的 agent）
termassist --config ./dev-config.toml   # 使用另一个配置文件
termassist read-pane       # 打印用户 pane 的屏幕 + 回滚历史
termassist read-pane --lines 50
termassist read-pane --pane right   # 改读 agent pane
termassist install-skill --scope user
```

`--config <path>` 会在所有涉及配置文件的地方替代默认的 per-OS 路径
（向导的存在性判定与写入目标、配置加载、向导提示文案里显示的路径）。
开发时很方便：指向仓库内的 `./dev-config.toml`（已被 git 忽略），就
不用动 `~/.config`。

- `read-pane` 通过本地 socket（Unix domain socket / Windows named pipe）
  连接正在运行的 TUI。socket 地址先取 `$TERM_ASSIST_SOCK`（已注入两侧
  pane 的环境变量），否则自动发现临时目录里最新的 socket；也可用
  `--socket <path>` 指定。读取进程已退出的 pane 会干净地报
  `pane closed` 错误。
- `install-skill` 把内置的 `SKILL.md` 写到指定位置（`--scope user` →
  `~/.agents/skills/termassist/`，`--scope project` →
  `./.agents/skills/termassist/`，`--path <dir>` → `<dir>/SKILL.md`）。
  启动时如果在两个常见位置都没找到 skill，termassist 会在进入 TUI 之前
  询问你是否安装。

## 配置

TOML 文件，位于各操作系统的配置目录（Linux 下为
`~/.config/termassist/config.toml`）。只有 `agent` 是必填项，其余字段
均可省略（以下为默认值）：

```toml
agent = "kimi"            # agent pane 的启动命令（可带参数）——必填
# shell = "/bin/zsh"      # 默认：$SHELL（Unix）/ %COMSPEC% 或 powershell（Windows）
layout = "horizontal"     # "horizontal"（左右）或 "vertical"（上下）
ratio = 0.5               # 左/上 pane 的占比，0.1..=0.9
scrollback_lines = 10000  # 每个 pane 的回滚行数上限

[keybindings]
focus_toggle = "Ctrl+g"
layout_toggle = "Ctrl+t"
scroll_mode = "Ctrl+s"
ratio_increase = "Ctrl+Right"
ratio_decrease = "Ctrl+Left"
toggle_agent = "Ctrl+n"
quit = "Ctrl+q"
```

键位语法：`Ctrl+`/`Alt+`/`Shift+` 修饰键加键名（`a`–`z`、`Enter`、`Esc`、
`Tab`、`Backspace`、`Space`、方向键、`Home`、`End`、`PageUp`、`PageDown`、
`Delete`、`Insert`、`F1`–`F12`）。

配置文件无效（语法错误、缺少 `agent`、类型不对）时会打印错误详情，
绝不静默回退默认值：交互式运行会启动向导重写配置（有效输入覆盖写入
并以 0 退出；空输入以非零退出）；非交互运行直接以非零状态码退出。

## 工作原理

```
┌─ termassist（单进程，ratatui 事件循环）──────────────────────┐
│  ┌─ Pane A ────────────┐   ┌─ Pane B ────────────┐          │
│  │ PTY → vt100 → screen│   │ PTY → vt100 → screen│          │
│  │  $SHELL             │   │  agent CLI          │          │
│  └──────────┬──────────┘   └──────────┬──────────┘          │
│             └──► ratatui 渲染层 ◄─────┘                     │
│  IPC server（本地 socket）◄── 读取 pane 屏幕内容            │
└─────────────────────────────────────────────────────────────┘
        ▲
        └── termassist read-pane（客户端）/ 嵌套的 `termassist`
```

- 每个 pane 是一个 PTY 子进程
  （[portable-pty](https://crates.io/crates/portable-pty)），输出喂给
  [vt100](https://crates.io/crates/vt100) 解析器；渲染层把 vt100 的屏幕
  状态 1:1 映射到 [ratatui](https://crates.io/crates/ratatui) 的 buffer
  （crossterm 后端）。
- `read-pane` 和嵌套的 `termassist` 都是本地 socket
  （[interprocess](https://crates.io/crates/interprocess)）上的轻量客户端。
- `vendor/vt100` 是 vt100 0.15.2 加两个 backport 的 panic 修复（深回滚
  窗口、`col_wrap` 下溢）——上游版本在回滚偏移超过屏幕高度时会 panic。

## 限制

- MVP 范围：没有多标签、没有会话持久化、没有插件系统。
- `agent` 命令按空格拆分参数，不支持引号转义。
- macOS/Windows 未实测（架构上平台代码已隔离在 `src/pty.rs` /
  `src/ipc.rs`）；Windows 下不支持 socket 自动发现，请用
  `TERM_ASSIST_SOCK` 或 `--socket` 指定。

## 许可证

MIT
