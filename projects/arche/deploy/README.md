# Arche SSH 部署

这套脚本把 Arche 仓库通过宿主 `sshd` 暴露给智能体，每个公钥只能访问一个仓库。

## 组成

```text
foundry-arche-stdio             forced command，校验客户端请求的仓库
foundry-arche-authorized-keys   AuthorizedKeysCommand，按公钥输出授权行
sshd_config.example             sshd 配置示例
install.sh                      安装脚本
```

## 安装

```bash
cargo build --release -p arche
sudo ./install.sh target/release/arche /srv/arche
```

安装脚本会：

- 创建 `foundry-arche` 服务账户；
- 安装 `arche` 和两个包装脚本到 `/usr/local/bin`；
- 创建 `/srv/arche` 和 `/etc/arche/keys`；
- 在可用时设置 POSIX ACL。

## 授权公钥

编辑 `/etc/arche/keys`：

```text
ssh-ed25519 AAAAC3Nza... project-a
ssh-ed25519 AAAAC3Nza... project-b
```

每行一个公钥和一个仓库名。

`foundry-arche-authorized-keys` 会输出：

```text
command="/usr/local/bin/foundry-arche-stdio /srv/arche project-a",no-port-forwarding,no-agent-forwarding,no-pty ssh-ed25519 AAAAC3Nza...
```

因此客户端无法请求授权之外的仓库。

## sshd

把 `sshd_config.example` 安装到：

```text
/etc/ssh/sshd_config.d/arche.conf
```

然后：

```bash
sshd -t
systemctl reload sshd
```

## 客户端

```bash
arche remote add ssh://foundry-arche@host/project-a
arche clone ssh://foundry-arche@host/project-a ./project-a
arche push
arche pull
```

客户端只执行：

```text
arche serve --stdio --root /srv/arche/project-a
```

实际仓库路径由 forced command 固定，客户端传入的路径只用于校验。

## 注意

- 不要在该 `Match` 块里设置 `ForceCommand`，否则会覆盖按公钥生成的命令；
- `foundry-arche-authorized-keys` 的 `AuthorizedKeysCommandUser` 必须是低权限账户；
- 仓库目录建议使用 `umask 0002` 和 POSIX ACL；
- `.arche/config` 不应保留过期 remote URL。
