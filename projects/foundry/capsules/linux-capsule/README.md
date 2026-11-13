# linux-capsule

一个独立的 Rust 项目：**`LinuxCapsule::desktop()` 或 `LinuxCapsule::headless()` 一次调用**，在 rootless Podman 容器里启动一个 Linux 运行环境，并返回一个可直接操作的 handle。

支持两种模式：

| 能力 | Desktop | Headless |
|---|---|---|
| 虚拟桌面（Xvfb + openbox） | ✅ | ❌ |
| 截图 / 鼠标 / 键盘 | ✅ | ❌ |
| 命令行操作 | ✅ | ✅ |
| 上传 / 下载文件 | ✅ | ✅ |
| 可写容器文件系统 | ✅ | ✅ |

Desktop 适合 GUI 应用；Headless 适合只需要命令、文件和网络的场景，不启动 X server，也没有 sidecar 端口和截图开销。

```
Desktop:
宿主
  └─ linux_capsule::LinuxCapsule
       ├─ Podman 容器（独立 mount/PID/net/IPC/user namespace）
       │    ├─ Xvfb :99
       │    ├─ openbox
       │    ├─ 你的应用
       │    └─ linux-capsule-sidecar  → 127.0.0.1:<随机端口>
       └─ screenshot / input / exec / upload / download / logs / stop

Headless:
宿主
  └─ linux_capsule::LinuxCapsule
       ├─ Podman 容器（独立 mount/PID/net/IPC/user namespace）
       │    └─ 你的命令（默认 `sleep infinity`）
       └─ exec / upload / download / logs / stop
```

容器共享宿主内核，不是硬安全边界；对抗性代码请用 microVM。

## 目录

```
capsules/linux-capsule/
  Cargo.toml                     # 独立 workspace，不属于 Foundry 主 workspace
  src/lib.rs                     # LinuxCapsule / Input / ExecOutput
  src/podman.rs                  # Podman CLI 后端
  src/sidecar_client.rs          # 宿主侧 HTTP 客户端
  src/bin/sidecar.rs             # 容器内截图/输入 sidecar（仅 Desktop）
  src/bin/linuxcapsule.rs        # demo 命令行
  images/linux-capsule/          # Xvfb + openbox + sidecar 基础镜像
  install.sh                     # 宿主机安装、subuid、网络、镜像构建
```

## 安装

在**普通用户**下运行（脚本只在装包和改 `/etc/subuid` 时使用 sudo）：

```bash
cd projects/foundry/capsules/linux-capsule
./install.sh
```

它会：

1. 安装 `podman`、`uidmap`、`slirp4netns`、`fuse-overlayfs`（可选装 CNI/netavark 插件）；
2. 检查并为当前用户分配不重叠的 `/etc/subuid`、`/etc/subgid`；
3. 执行 `podman system migrate`，验证 rootless 和 `podman unshare`；
4. 创建 Podman 网络 `linux-capsule`；
5. 用 `images/linux-capsule/Containerfile` 构建 `localhost/linux-capsule:1`（多阶段构建，sidecar 在镜像内编译）；
6. 跑两个 smoke test：Desktop 的 sidecar 健康检查，以及 Headless 的 `exec` + `upload/download`。

常用选项：

```bash
./install.sh --rebuild          # 重新构建镜像
./install.sh --skip-packages    # 只配 subuid/网络/镜像
./install.sh --skip-image       # 只配宿主环境
./install.sh --linger           # 允许容器在退出登录后继续运行
./install.sh --image my/image:tag --network my-net
```

如果 Docker Hub 拉取慢，先配置 `/etc/containers/registries.conf`，或用 `RUST_IMAGE` / `BASE_IMAGE` 环境变量指定镜像：

```bash
RUST_IMAGE=mirror.example/rust:1-bookworm \
BASE_IMAGE=mirror.example/debian:bookworm-slim \
./install.sh
```

## 验证

```bash
# Desktop：截图 + 点击 + 打字 + exec + 上传/下载
cargo run --bin linuxcapsule -- demo --mode desktop

# Headless：exec + 上传/下载，不启动 Xvfb
cargo run --bin linuxcapsule -- demo --mode headless
```

Desktop demo 会保存 `/tmp/linux-capsule-before.png` 和 `/tmp/linux-capsule-after.png`，并在屏幕中心点击、输入。
两种模式都会：

- 上传 `/tmp/linux-capsule-upload.txt` 到容器；
- 在容器里 `cat` 出来；
- 在容器里写 `/tmp/linux-capsule-download.txt` 并下载到宿主；
- 执行 `uname -a`；
- 停止并删除容器。

## 库用法：一个对象，方法平铺

### Desktop

```rust
use linux_capsule::{Input, LinuxCapsule};

#[tokio::main]
async fn main() -> linux_capsule::Result<()> {
    let capsule = LinuxCapsule::desktop().await?; // 默认带 xterm

    let shot = capsule.screenshot().await?;       // PNG
    std::fs::write("before.png", &shot)?;

    capsule.input(Input::Click { x: 640, y: 400, button: 1 }).await?;
    capsule.input(Input::Text { text: "hello".into() }).await?;
    capsule.input(Input::Key { key: "Return".into() }).await?;

    let shot = capsule.screenshot().await?;
    std::fs::write("after.png", &shot)?;

    capsule.close().await?;
    Ok(())
}
```

需要其他应用时，先 `desktop()`，再用 `exec("DISPLAY=:99 your-app &")` 启动。

### Headless

```rust
use linux_capsule::LinuxCapsule;

#[tokio::main]
async fn main() -> linux_capsule::Result<()> {
    let capsule = LinuxCapsule::headless().await?;

    let output = capsule.exec("uname -a").await?;
    println!("{}", output.stdout_string());

    capsule.upload(std::path::Path::new("local.txt"), "/tmp/local.txt").await?;
    capsule.exec("echo hello > /tmp/hello.txt").await?;
    capsule.download("/tmp/hello.txt", std::path::Path::new("hello.txt")).await?;

    capsule.close().await?;
    Ok(())
}
```

### 日常方法

| 方法 | 说明 |
|---|---|
| `desktop()` | 创建 Desktop capsule，默认带 xterm |
| `headless()` | 创建 Headless capsule，默认 `sleep infinity` |
| `screenshot()` | 截图，返回 PNG `Vec<u8>` |
| `input(Input)` | 移动、点击、滚轮、按键、文本 |
| `exec(script)` | 容器内执行 shell 脚本 |
| `upload(local, remote)` / `download(remote, local)` | 文件传输 |
| `close()` | 停止并删除 capsule |

输入统一走一个 `Input`：

| `Input` | 说明 |
|---|---|
| `Move { x, y }` | 移动鼠标 |
| `Click { x, y, button }` | 点击，1=左键、2=中键、3=右键 |
| `Scroll { x, y, delta }` | 滚轮，正数向下 |
| `Key { key }` | 按键，如 `Return`、`Tab`、`Left` |
| `Text { text }` | 输入可打印 ASCII 文本 |

### 固定默认值

这个最小 API 不暴露逐 capsule 配置：

| 项 | 固定值 |
|---|---|
| 镜像 | `localhost/linux-capsule:1` |
| 网络 | `linux-capsule` |
| Desktop 屏幕 | 1280×800×24 |
| 宿主挂载 | 无 |
| 容器资源 | 2048 MB 内存 / 2 CPU / 512 pids |

`desktop()` 默认启动 xterm；需要其他应用时在创建后用 `exec("DISPLAY=:99 app &")`。
`headless()` 默认 `sleep infinity`；命令、文件和网络都通过 `exec` / `upload` / `download` 操作。

`desktop()` / `headless()` 会等待 sidecar 或容器就绪，再等待 250 ms 后返回。

### 网络

- 所有 capsule 加入固定的 `linux-capsule` 网络；
- 同一网络内的容器可以用容器名互访（Podman 4.x 的 aardvark-dns 最稳；Podman 3.4 需先验证）；
- 访问宿主：`host.containers.internal`；
- 出站默认走 Podman 的 NAT；
- 最小 API 不发布应用端口到宿主；需要宿主访问容器内服务时，需要扩展 API 或自行用 `podman` 操作。

注意：如果宿主服务只监听 `127.0.0.1`，容器通过 bridge gateway 是连不上的；需要让服务监听 bridge IP/`0.0.0.0`，或走 Unix socket/代理。

### 隔离默认值

- `--cap-drop ALL`
- `--security-opt no-new-privileges`
- `--memory 2048m`、`--cpus 2`、`--pids-limit 512`
- `/tmp`、`/run` 使用带大小限制的 tmpfs
- 不挂载宿主 X11 socket、docker socket 或家目录
- 容器的 writable layer 独立，`close()` 后删除

## 已知限制

- 容器共享内核，不是绝对安全边界；不可信代码用 microVM/gVisor。
- Ubuntu 22.04 自带 Podman 3.4，rootless 多容器 DNS/互访能力有限；要求容器间按名字互访时建议 Podman 4.x 或 Docker。
- 默认设置 2048 MB 内存、2 CPU、512 pids；rootless 下要求 systemd 已委派 cpu/memory/pids 控制器，否则容器启动会报 `cgroup controller ... is not available`。
- Desktop sidecar 的键盘映射按 Xvfb 默认的 US 布局；文本输入只支持可打印 ASCII。
- Headless 模式不启动 Xvfb，因此 `screenshot()` / `input()` 返回 `Error::NoDesktop`；`exec`、`upload/download` 仍然可用。
- `desktop()` 默认启动 xterm；`headless()` 默认 `sleep infinity`。没有逐 capsule 的 `app`、镜像、网络、资源、端口、`setup`、`files` 配置。
- 镜像同时包含桌面依赖；Headless 运行时不会启动 X，但镜像体积仍然较大。
- 每次创建都会重新启动容器；需要毫秒级启动可在此基础上加预热池。
