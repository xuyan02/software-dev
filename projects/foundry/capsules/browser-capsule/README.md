# browser-capsule

一个独立的 Rust 项目：**`BrowserCapsule::create()`** 在 rootless Podman 容器里启动 Chromium，并返回一个可直接操作的 handle。

宿主通过 Chrome DevTools Protocol（CDP）导航、截图和输入。BrowserCapsule 不提供 shell 和文件传输。

```
宿主
  └─ browser_capsule::BrowserCapsule
       └─ CDP（WebSocket + HTTP，127.0.0.1:<随机端口>）
            ▼
          Podman 容器
            └─ Chromium --headless=new --remote-debugging-port=9222
```

容器共享宿主内核，不是硬安全边界；对抗性代码请用 microVM。

## 目录

```
capsules/browser-capsule/
  Cargo.toml                     # 独立 workspace
  src/lib.rs                     # BrowserCapsule / Input
  src/cdp.rs                     # CDP 客户端
  src/podman.rs                  # Podman CLI 后端
  src/bin/browsercapsule.rs      # demo 命令行
  images/browser-capsule/        # Chromium 镜像
  install.sh                     # 宿主机安装、subuid、网络、镜像构建
```

## 安装

普通用户执行：

```bash
cd projects/foundry/capsules/browser-capsule
./install.sh
```

它会：

1. 安装 `podman`、`uidmap`、`slirp4netns`、`fuse-overlayfs`；
2. 配置并验证 rootless `subuid/subgid`；
3. 创建 Podman 网络 `browser-capsule`；
4. 构建 `localhost/browser-capsule:1`（Debian + Chromium）；
5. 启动临时容器，检查 CDP `/json/version`。

常用选项：

```bash
./install.sh --rebuild
./install.sh --skip-packages
./install.sh --skip-image
./install.sh --linger
```

如果 Docker Hub 不可达（连接被 reset），用可访问的镜像源构建：

```bash
BASE_IMAGE=docker.m.daocloud.io/library/debian:bookworm-slim ./install.sh
# 或
BASE_IMAGE=docker.1ms.run/library/debian:bookworm-slim ./install.sh
```

如果 Debian apt 下载也慢，再加一个 apt 镜像：

```bash
BASE_IMAGE=docker.m.daocloud.io/library/debian:bookworm-slim \
DEBIAN_MIRROR=mirrors.tuna.tsinghua.edu.cn \
./install.sh
```

也可以先 `podman pull` 这些镜像验证网络，再执行 install.sh。

## 验证

```bash
cargo run --bin browsercapsule -- demo --url https://example.com
```

demo 会：

- 创建容器并导航到页面；
- 保存 `/tmp/browser-capsule-before.png`；
- 点击、输入文本、按 Return；
- 保存 `/tmp/browser-capsule-after.png`；
- 关闭并删除容器。

## 库用法

```rust
use browser_capsule::{BrowserCapsule, Input};

#[tokio::main]
async fn main() -> browser_capsule::Result<()> {
    let capsule = BrowserCapsule::create().await?;
    capsule.navigate("https://example.com").await?;

    let png = capsule.screenshot().await?;
    std::fs::write("page.png", &png)?;

    capsule.input(Input::Click { x: 640, y: 400, button: 1 }).await?;
    capsule.input(Input::Text { text: "hello".into() }).await?;
    capsule.input(Input::Key { key: "Return".into() }).await?;

    capsule.navigate("https://example.org").await?;

    capsule.close().await?;
    Ok(())
}
```

## 方法

| 方法 | 说明 |
|---|---|
| `create()` | 启动容器、连接 CDP，初始页面为 `about:blank` |
| `navigate(url)` | 导航并等待 `document.readyState == complete` |
| `screenshot()` | 当前 viewport 的 PNG `Vec<u8>` |
| `input(Input)` | 鼠标/键盘事件 |
| `close()` | 停止并删除容器 |

BrowserCapsule **不提供** `exec`、`upload`、`download`：浏览器胶囊只通过 CDP 做 UI 操作。需要在隔离环境里跑命令或传文件时，使用 [`linux-capsule`](../linux-capsule/)。

输入统一走 `Input`：

| `Input` | 说明 |
|---|---|
| `Move { x, y }` | 移动鼠标 |
| `Click { x, y, button }` | 点击，1=左键、2=中键、3=右键 |
| `Scroll { x, y, delta }` | 滚轮，正数向下 |
| `Key { key }` | 按键，如 `Return`、`Tab`、`Left`、`F5` |
| `Text { text }` | 输入文本 |

坐标是 viewport 坐标，与截图尺寸一致（固定 1280×800，`deviceScaleFactor=1`）。

## 固定默认值

| 项 | 固定值 |
|---|---|
| 镜像 | `localhost/browser-capsule:1` |
| 网络 | `browser-capsule` |
| Viewport | 1280×800 |
| 资源 | 2 GB 内存 / 2 CPU / 512 pids |
| CDP 端口 | 宿主 `127.0.0.1:<随机端口>` → 容器 `9222` |
| 宿主挂载 | 无 |

最小 API 不暴露镜像、网络、viewport、资源、启动参数等配置。

## 限制

- Chromium 在容器内使用 `--no-sandbox`；隔离边界是 Podman 容器，不是 Chrome sandbox。
- CDP 没有鉴权，只绑定 `127.0.0.1` 的随机端口；同一宿主上的本地进程可以控制它，不要暴露到网络。
- 截图只支持 PNG。
- `navigate` 等待 `readyState == complete`；单页应用可能在之后继续异步渲染，需要调用方自行等待或用截图轮询。
- 不提供 shell 和文件传输；这是浏览器胶囊的边界，不是遗漏。
- 容器共享宿主内核，不是绝对安全边界。

## 测试应用：打开百度并截图

先完成安装和镜像构建（见上文），然后：

```bash
cargo run --example baidu -- /tmp/baidu.png
```

它会：

1. 创建浏览器容器；
2. 打开 `https://www.baidu.com`；
3. 等待 2 秒让页面完成异步渲染；
4. 截图并保存到 `/tmp/baidu.png`；
5. 关闭并删除容器。

如果还没有运行 `./install.sh` 或没有 Podman/镜像，`create()` 会返回 `Unavailable`。
