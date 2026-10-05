# Linux 一键部署与维护

本页适用于羽迹探针 Rust 独立版 0.10.3。安装器下载预编译发行包，不在用户服务器上编译 Rust，不需要数据库或 PHP。

## 安装

支持 Debian 12/13、Ubuntu 22.04/24.04/26.04、Rocky/AlmaLinux 9/10、CentOS Stream 9/10、Fedora 42/43/44，架构为 amd64 或 arm64，需要 systemd 和 root。

~~~sh
curl -fL --proto '=https' --proto-redir '=https' --tlsv1.2 \
  https://raw.githubusercontent.com/coexacx/yuji-probe-rust/v0.10.3/install.sh \
  -o /root/yuji-install.sh
bash /root/yuji-install.sh
~~~

只需填写站点名称。脚本安装运行依赖、验证发行签名、创建独立服务账户和随机管理员密码，结束后显示 IP:端口、管理员和初始密码。

默认端口 19281。需要其他端口可运行 bash /root/yuji-install.sh --port 29281。运行 --check 仅检查系统与架构，不安装。需要自动化时可用 --site-name '站点名称'。

已有宝塔或其他站点不影响使用；安装器不占用 80/443，不安装 Nginx，不申请证书。只检查自己的目录、账户、服务名及端口；发现既有数据会停止覆盖。已通过本脚本安装的实例，再次运行脚本会进入管理菜单。

## 首次访问

用安装结束显示的 HTTP 地址查看面板，初始管理员为 admin，密码随机生成。初始凭据保存在 /etc/yuji-probe/initial-admin.json，仅 root 可读。更改密码后可以删除此文件。

HTTP 不加密。添加节点与使用终端前，按 [反向代理文档](反向代理.md) 自行配置域名、Nginx 和证书，再在管理菜单中设置 HTTPS 入口。主控与 Agent 始终使用 WSS，不因初始 HTTP 访问入口而降级。

## 管理菜单

~~~sh
sudo yuji-probe
~~~

顶部显示版本、运行状态和地址。菜单提供：

| 编号 | 操作 |
| --- | --- |
| 1 | 从本仓库检查新版，确认后签名验证、更新、健康检查 |
| 2 / 3 / 4 | 重启、启动、停止主控 |
| 5 / 6 | 日志、systemd 运行详情 |
| 7 | 空面板设置 HTTP IP:端口或 HTTPS 地址 |
| 8 | 创建包含状态、密钥及管理配置的本机备份 |
| 9 | 停止服务后使用专用命令重置二步验证，再恢复原运行状态 |
| 10 | 打开反向代理文档地址 |
| 11 | 回退到上次升级前的程序和数据，需要明确确认 |
| 12 | 卸载程序和管理服务，保留面板数据 |
| 0 | 退出 |

菜单兼容窄终端，NO_COLOR=1 可关闭颜色。日志默认显示最近 80 行，不会让菜单停留在持续刷新的日志流中。

命令行也可以运行 yuji-probe summary、yuji-probe restart、yuji-probe backup。现有宝塔手动安装的服务路径不同，不要将此菜单直接用于管理旧实例；继续使用后台的签名更新与对应 systemd 服务。

## 路径与备份

| 用途 | 路径 |
| --- | --- |
| 程序 | /opt/yuji-probe |
| 私有状态 | /var/lib/yuji-probe/control |
| 安装配置 | /etc/yuji-probe |
| 系统服务 | yuji-probe.service |
| 管理命令 | /usr/local/bin/yuji-probe |
| 本机备份 | /var/backups/yuji-probe |
| 签名更新器 | /etc/yuji-probe-rust-updaters/main.json |

本机备份会短暂停止主控以取得一致状态，然后恢复原运行状态；备份只允许 root 读取，内含密钥，不应放进网站目录。跨服务器迁移优先使用后台的加密备份、恢复与迁移流程。程序卸载后保留数据，不会清除被控服务器。

## 更新

菜单的更新入口与网页后台使用同一个签名更新器。保留现有账户、节点、Agent 密钥、主题和通知配置。失败会恢复原程序和状态。旧版宝塔手动部署更新到 0.10.3 后仍维持原 HTTPS 和回环监听，不会自动开放公网端口。

升级前也可主动创建备份。回退将恢复升级前的数据，升级之后新增的配置不会被带回旧版本。

## 离线发行包

可把同版本的 panel-stable.json 与完整 ZIP 放在 root 所有、不可被其他用户写入的目录，然后运行：

~~~sh
bash install.sh --release-dir /root/yuji-release --site-name '羽迹探针'
~~~

离线包也必须通过 Ed25519 签名、大小、SHA-256 和解压路径校验。运行依赖仍需提前安装或由系统包管理器获取。
