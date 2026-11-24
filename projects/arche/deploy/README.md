# Arche SSH 部署

这套脚本把 Arche 仓库通过宿主 `sshd` 暴露给智能体，每个公钥只能访问一个仓库。

## 组成

```text
foundry-arche-stdio             forced command，校验客户端请求的仓库
foundry-arche-authorized-keys   AuthorizedKeysCommand，按公钥输出授权行
sshd_config.example             sshd 配置示例
install.sh                      安装脚本
install-sshd.sh                 安装 sshd drop-in 并 reload
```

## 安装

Foundry 默认把设计空间放在 `~/.foundry/design-spaces`，并自动生成
`~/.foundry/arche-sshd/authorized_keys`。仓库根应指向同一目录：

```bash
cargo build --release -p arche
sudo ./install.sh target/release/arche /home/xuyan/.foundry/design-spaces
sudo ./install-sshd.sh /home/xuyan/.foundry/arche-sshd/authorized_keys "$USER"
```

安装脚本会：

- 创建 `foundry-arche` 服务账户（shell 为 `/bin/sh`，forced command 仍受限）；
- 安装 `arche` 和两个包装脚本到 `/usr/local/bin`；
- 创建仓库根和 `/etc/arche/keys`；
- 在可用时设置 POSIX ACL，包括让服务账户穿过操作员主目录的 ACL。

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

`install-sshd.sh` 会写入 `/etc/ssh/sshd_config.d/20-foundry-arche.conf`，
校验配置并 reload。也可以手工参考 `sshd_config.example`。

`AuthorizedKeysCommandUser` 需要对控制面生成的文件有读权限；脚本默认使用
sudo 调用者（通常是操作员账户）。不要把 `AuthorizedKeysCommandUser` 设为
不能穿过操作员主目录的服务账户。

## 客户端

```bash
arche remote add ssh://foundry-arche@host/project-a
arche clone ssh://foundry-arche@host/project-a ./project-a
arche push
arche pull
```

clone 会从远程仓库继承已记录的源码 Git URL，不需要再传 `--source`。

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

## Foundry 集成

Foundry 新项目会在 `design_space_root` 下创建 Arche 仓库，并由控制平面动态写入
SSH 公钥。接入宿主 sshd 时：

1. 安装 `foundry-arche-stdio`（本目录的 `install.sh`，或手动复制到
   `/usr/local/bin`）；
2. 让 `foundry-arche` 服务账户能读写 Foundry 的 `design_space_root`（建议 POSIX ACL）；
3. 在宿主 sshd 的 `Match User foundry-arche` 块中，把 `AuthorizedKeysCommand`
   指向 Foundry 生成的 `~/.foundry/arche-sshd/authorized_keys`。可以复用
   `foundry-arche-authorized-keys`：

   ```text
   Match User foundry-arche
       AuthorizedKeysCommand /usr/local/bin/foundry-arche-authorized-keys %u %k /home/<operator>/.foundry/arche-sshd/authorized_keys
       AuthorizedKeysCommandUser <authorized keys user>
       PasswordAuthentication no
   ```

4. 在 `~/.foundry/foundry.toml` 中设置
   `arche_ssh_url = "ssh://foundry-arche@<host>"`。

Foundry 写出的行形如：

```text
restrict,command="/usr/local/bin/foundry-arche-stdio '/home/<operator>/.foundry/design-spaces' '<project-id>'" ssh-ed25519 AAAA...
```
