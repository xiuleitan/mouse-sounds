<h1 align="center">
  <span style="color: transparent; background: linear-gradient(120deg, #FF6B6B, #4ECDC4, #45B7D1); -webkit-background-clip: text; background-clip: text;">Mouse Sounds</span>
</h1>

<p align="center">
  <img src="https://img.shields.io/badge/Platform-Linux-blue?style=for-the-badge&logo=linux" alt="Platform Linux">
  <img src="https://img.shields.io/badge/Language-Rust-orange?style=for-the-badge&logo=rust" alt="Language Rust">
  <img src="https://img.shields.io/badge/Status-Active-success?style=for-the-badge" alt="Status Active">
</p>

> 一个用于 Linux 的高颜值小工具：监听全局鼠标按下/松开事件并播放清脆的点击音效。

### 🎧 音效演示

*点击下方音频控件播放，听一下默认的清脆效果吧！*

**鼠标按下** (`click_down.wav`):  
<audio src="./click_down.wav" controls></audio>

**鼠标松开** (`click_up.wav`):  
<audio src="./click_up.wav" controls></audio>

---

- 鼠标按下播放 `click_down.wav`
- 鼠标松开播放 `click_up.wav`
- 默认自动监听所有可读的鼠标输入设备（`/dev/input/event*`）

## 1. 小程序作用

适用于你想要“系统级鼠标点击反馈音”的场景，例如：

- 桌面环境没有内置点击音
- 想自定义更短/更清脆的点击音
- 需要在 Wayland 下也能捕获全局鼠标按键事件

## 2. 安装步骤

### 2.1 环境要求

- Linux（Wayland / X11 均可）
- Rust 工具链（建议 stable）
- 可用音频输出（PipeWire / PulseAudio / ALSA）
- 当前用户对 `/dev/input/event*` 有读取权限（Wayland 部分见下文）

### 2.2 从源码构建

```bash
cargo build --release
```

生成二进制：

`target/release/mouse-sounds`

### 2.3 可选：安装到用户命令路径

方式 1（推荐）：

```bash
cargo install --path .
```

安装后可直接使用：

`~/.cargo/bin/mouse-sounds`

方式 2（手动复制）：

```bash
install -Dm755 target/release/mouse-sounds ~/.local/bin/mouse-sounds
```

### 2.4 使用安装包（推荐给最终用户）

先在项目根目录打包：

```bash
./scripts/package.sh
```

默认会生成：

- `dist/mouse-sounds-<version>-<target>.tar.gz`
- `dist/mouse-sounds-<version>-<target>.tar.gz.sha256`

可选先校验文件完整性：

```bash
cd dist
sha256sum -c mouse-sounds-<version>-<target>.tar.gz.sha256
```

在目标机器上安装：

```bash
tar -xzf mouse-sounds-<version>-<target>.tar.gz
cd mouse-sounds-<version>-<target>
./install.sh
```

安装完成后可直接启动用户服务：

```bash
systemctl --user daemon-reload
systemctl --user enable --now mouse-sounds.service
```

如果你暂时不想启用服务，也可以手动运行：

```bash
~/.local/bin/mouse-sounds run --config ~/.config/mouse-sounds/config.toml
```

## 3. 配置方法

默认情况下，程序会在当前工作目录读取：

- `click_down.wav`
- `click_up.wav`

你也可以使用 TOML 配置文件覆盖默认值，例如 `config.toml`：

```toml
[sounds]
down = "/home/your_user/MouseSounds/click_down.wav"
up = "/home/your_user/MouseSounds/click_up.wav"

[device]
# 留空或不写：自动监听所有可读的鼠标设备
# 指定路径：只监听某一个 event 设备
event_path = ""

[behavior]
# true: 所有鼠标按键
# false: 仅左键
all_buttons = true
```

配置项说明：

- `sounds.down`：按下音效文件路径
- `sounds.up`：松开音效文件路径
- `device.event_path`：指定单一输入设备（可选）
- `behavior.all_buttons`：是否监听所有鼠标按钮

## 4. 使用方法

### 4.1 启动前检查

```bash
cargo run -- check
```

或（release）：

```bash
./target/release/mouse-sounds check
```

检查项包括：

- 音效文件可读且格式可解码
- 是否存在可监听的鼠标输入设备
- 输出当前可用设备列表（路径 + 名称）

### 4.2 运行

默认运行：

```bash
cargo run
```

等价于：

```bash
cargo run -- run
```

使用配置文件运行：

```bash
cargo run -- run --config /path/to/config.toml
```

release 运行：

```bash
./target/release/mouse-sounds run --config /path/to/config.toml
```

## 5. Wayland 等特殊情况说明

### 5.1 为什么 Wayland 需要额外配置

Wayland 下普通应用通常不能直接拿到全局输入事件。  
本程序通过读取 `/dev/input/event*` 工作，所以关键在于给“当前用户”授予最小必要读权限。

### 5.2 推荐权限配置（udev + 用户组）

1. 创建用户组：

```bash
sudo groupadd --force inputread
```

2. 将当前用户加入组：

```bash
sudo usermod -aG inputread "$USER"
```

3. 新建规则 `/etc/udev/rules.d/99-inputread.rules`：

```bash
KERNEL=="event*", SUBSYSTEM=="input", GROUP="inputread", MODE="0640"
```

4. 重载规则：

```bash
sudo udevadm control --reload-rules
sudo udevadm trigger
```

5. 重新登录（或重启）使组权限生效。

说明：

- 不建议常驻 `root` 运行。即使能读输入设备，也容易引入更高安全风险。
- `root`/system 级服务还可能拿不到当前桌面音频会话，导致“有监听无声音”。

### 5.3 建议长期运行方式（systemd --user）

为当前用户创建服务文件：

`~/.config/systemd/user/mouse-sounds.service`

```ini
[Unit]
Description=Mouse click sound daemon
After=graphical-session.target
Wants=graphical-session.target

[Service]
ExecStart=%h/.cargo/bin/mouse-sounds run --config %h/.config/mouse-sounds/config.toml
Restart=always
RestartSec=2

[Install]
WantedBy=default.target
```

启用并启动：

```bash
systemctl --user daemon-reload
systemctl --user enable --now mouse-sounds.service
```

查看状态和日志：

```bash
systemctl --user status mouse-sounds.service
journalctl --user -u mouse-sounds.service -f
```

注意：

- `systemd --user` 环境下，建议在配置文件中使用音效文件绝对路径。
- 先运行一次 `mouse-sounds check`，确认设备和音效均正常后再启用常驻服务。

## 6. 常见问题

1. `no readable mouse input device found under /dev/input/event*`  
通常是权限未生效或尚未重新登录，优先检查用户组和 udev 规则。

2. 程序有日志但没有声音  
检查当前用户的音频会话、默认输出设备是否正常；确认音效文件可播放。

3. 只想监听某一个鼠标  
在 `device.event_path` 中指定目标设备路径，例如 `/dev/input/event12`。
