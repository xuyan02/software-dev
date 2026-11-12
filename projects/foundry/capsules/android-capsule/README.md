# android-capsule

一个独立的 Rust 项目：**`AndroidCapsule::create()`** 在 rootless Podman 容器里启动 Android Emulator，并返回一个可直接操作的 handle。

宿主通过 `adb` 截图、点击/滑动、输入文本、安装 APK、启动应用。

```
宿主
  └─ android_capsule::AndroidCapsule
       └─ adb（127.0.0.1:<随机端口>）
            ▼
          Podman 容器（rootless）
            └─ Android Emulator（/dev/kvm）
```

容器共享宿主内核，不是硬安全边界；对抗性代码请用 microVM。

## 前提

- Linux 宿主机，`/dev/kvm` 可读可写；
- rootless Podman；
- 宿主机安装 `adb`；
- 约 5 GB 磁盘给 Android SDK、系统镜像和容器存储；
- 首次启动需要 1–3 分钟（软件 GPU + 完整 Android 启动）。

不需要 privileged 容器，也不需要 `binder_linux`；模拟器使用 KVM。

## 安装

普通用户执行：

```bash
cd projects/foundry/capsules/android-capsule
./install.sh
```

它会：

1. 检查 `/dev/kvm`；
2. 安装 `podman`、`uidmap`、`adb`；
3. 配置并验证 rootless `subuid/subgid`；
4. 构建 `localhost/android-capsule:1`（Debian + Android SDK + Emulator + system image）；
5. 启动临时容器，等待 Android 启动完成并做一次 `screencap`。

如果 Docker Hub 不可达：

```bash
BASE_IMAGE=docker.m.daocloud.io/library/debian:bookworm-slim \
DEBIAN_MIRROR=mirrors.tuna.tsinghua.edu.cn \
./install.sh
```

Android SDK 和系统镜像从 `dl.google.com` 下载；如果访问困难，需要给宿主机或 Podman build 配代理。

## 验证

```bash
cargo run --bin androidcapsule -- demo
```

demo 会：

- 创建 Android Emulator；
- 保存 `/tmp/android-capsule-before.png`；
- 启动 `com.android.settings`；
- 保存 `/tmp/android-capsule-after.png`；
- 关闭并删除容器。

## 库用法

```rust
use android_capsule::{AndroidCapsule, Input};

#[tokio::main]
async fn main() -> android_capsule::Result<()> {
    let capsule = AndroidCapsule::create().await?;

    let png = capsule.screenshot().await?;
    std::fs::write("screen.png", &png)?;

    capsule.input(Input::Tap { x: 540, y: 1200 }).await?;
    capsule.input(Input::Swipe {
        x1: 540, y1: 1600,
        x2: 540, y2: 600,
        duration_ms: 300,
    }).await?;
    capsule.input(Input::Text { text: "hello".into() }).await?;
    capsule.input(Input::Key { key: "Back".into() }).await?;

    capsule.install_apk(std::path::Path::new("app.apk")).await?;
    capsule.launch("com.example.app").await?;

    capsule.close().await?;
    Ok(())
}
```

## 方法

| 方法 | 说明 |
|---|---|
| `create()` | 启动模拟器并等待 `sys.boot_completed == 1` |
| `screenshot()` | 当前屏幕的 PNG `Vec<u8>` |
| `input(Input)` | Tap / Swipe / Text / Key |
| `install_apk(local)` | `adb install -r` |
| `launch(package)` | 启动包的默认 launcher activity |
| `close()` | 停止模拟器并删除容器 |

输入：

| `Input` | 说明 |
|---|---|
| `Tap { x, y }` | 点击屏幕坐标 |
| `Swipe { x1, y1, x2, y2, duration_ms }` | 滑动 |
| `Text { text }` | 输入单行文本，空格会转成 `%s` |
| `Key { key }` | `Home`、`Back`、`Return`、`Tab`、`ArrowUp` 等 |

坐标与截图尺寸一致（Pixel AVD，默认 1080×2340）。

## 固定默认值

| 项 | 固定值 |
|---|---|
| 镜像 | `localhost/android-capsule:1` |
| Android API | 34 |
| System image | `system-images;android-34;google_apis;x86_64` |
| AVD | `pixel` |
| 容器资源 | 4 GB 内存 / 2 CPU / 1024 pids |
| Emulator 内存 | 2048 MB |
| ADB 端口 | 宿主 `127.0.0.1:<随机端口>` → 容器 `5555` |
| 网络 | Podman 默认 rootless 网络 |

最小 API 不暴露镜像、API、AVD、分辨率、资源等配置。

## 限制

- 需要 `/dev/kvm`；没有 KVM 时只能软件模拟，速度不可接受。
- 使用 x86_64 system image；带 ARM native 库的 APK 不能直接运行。
- 软件 GPU（`swiftshader_indirect`），3D/视频性能有限。
- 首次启动和冷启动较慢，通常 1–3 分钟。
- 不提供 shell、`exec`、上传/下载；唯一的文件入口是 `install_apk`。
- ADB 没有鉴权，只绑定 `127.0.0.1` 的随机端口。
- 容器共享宿主内核，不是绝对安全边界。
