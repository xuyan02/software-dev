# Capsules

`Capsule` 表示**一个自包含的虚拟运行环境**：有独立的文件系统、网络和生命周期，可以运行应用，并通过统一的 handle 操作（截图、输入、exec、上传/下载）。

它不是“容器”，也不是“沙箱”的同义词。容器、microVM、进程只是 Capsule 的隔离后端；UI 类型是 Capsule 的种类。

## 三个独立项目

| 项目 | UI 类型 | 状态 | 典型后端 |
|---|---|---|---|
| [`linux-capsule/`](linux-capsule/) | Linux 桌面 / 命令行 | **已实现** | rootless Podman |
| [`browser-capsule/`](browser-capsule/) | 浏览器 | **已实现** | Chromium + CDP + rootless Podman |
| [`android-capsule/`](android-capsule/) | Android 应用 | **已实现** | Android Emulator + KVM + rootless Podman |

它们是**三个不同的项目**，各自有独立的 workspace、镜像、安装脚本和 CLI；暂不提前抽公共 crate，等两个以上项目出现真实重复再抽。

## 命名规则

```
<UI 类型> Capsule        LinuxCapsule / BrowserCapsule / AndroidCapsule
<隔离技术> Backend       PodmanBackend / MicroVmBackend
```

- `Capsule` 描述“运行什么、怎么操作”；
- `Backend` 描述“怎么隔离”；
- 不再用 `sandbox` 或 `container` 给 UI 类型命名；
- 避免 `Session`（和登录/浏览器 session 冲突）、`Stage`（和 workflow stage 冲突）、`Runtime`（和语言/容器 runtime 冲突）。

## LinuxCapsule

当前实现位于 [`linux-capsule/`](linux-capsule/)，支持：

- **Desktop**：Xvfb + openbox + GUI 应用，截图/鼠标/键盘；
- **Headless**：不启动 X，只运行命令，支持 `exec`、上传/下载、网络；
- 两种模式都支持可写文件系统；镜像、网络和资源使用固定默认值。

详见 [`linux-capsule/README.md`](linux-capsule/README.md)。

## BrowserCapsule

当前实现位于 [`browser-capsule/`](browser-capsule/)，支持：

- `create()` / `navigate(url)`；
- `screenshot()`（PNG）和 `input(Input)`，通过 CDP 控制 Chromium；
- `close()`；
- 不提供 shell 和文件传输；
- 镜像、viewport 和资源使用固定默认值；网络使用 Podman 默认 rootless 网络。

详见 [`browser-capsule/README.md`](browser-capsule/README.md)。

## AndroidCapsule

当前实现位于 [`android-capsule/`](android-capsule/)，支持：

- `create()`；
- `screenshot()`（PNG）和 `input(Input)`，通过 `adb` 控制 Android；
- `install_apk()` / `launch()`；
- `close()`；
- 不提供通用 shell、上传/下载；
- rootless Podman + `/dev/kvm` + Android Emulator，镜像、API、AVD 和资源使用固定默认值。

详见 [`android-capsule/README.md`](android-capsule/README.md)。

## 共享约定

不同 Capsule 项目之间只约定行为和命名，不共享实现：

- 每个项目有自己的创建入口（LinuxCapsule: `desktop()` / `headless()`；BrowserCapsule: `create()`；AndroidCapsule: `create()`）；
- 公共生命周期：`close()`；
- LinuxCapsule 额外提供 `exec`、`upload`、`download`；
- BrowserCapsule 提供 `navigate`、`screenshot`、`input`，不提供 shell 和文件传输；
- AndroidCapsule 提供 `screenshot`、`input`、`install_apk`、`launch`，不提供通用 shell 和文件传输；
- 每个项目自己的 `install.sh` 负责宿主机依赖、镜像构建和 smoke test；
- 每个项目自己的 README 说明模式、限制和验证方法。
