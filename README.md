# 羽迹探针 · Rust 独立版

Nginx + Rust 单二进制服务器探针。页面、网页安装向导、API 与 WSS 均内嵌在主控程序中，运行无需 PHP-FPM、数据库、Node.js 或 Rust 编译环境。

[下载完整安装包](https://github.com/coexacx/yuji-probe-rust/releases/latest) · [一键安装与宝塔部署](docs/Nginx-Rust部署教程.md) · [从原仓库迁移](docs/仓库迁移-0.10.1.md) · [构建源码](ops/BUILD.md)

本仓库独立维护 Nginx + Rust 版。原 [Nginx + PHP + Rust 项目](https://github.com/coexacx/yuji-probe) 保留自己的发行与更新通道。本仓库的安装器、主控更新、Agent 下载全部使用 **coexacx/yuji-probe-rust**。

## 功能

- 公开看板：全球国家与国旗、CPU、内存、已启用 Swap、磁盘、网卡实时速率和累计上下行。
- 管理后台：自动部署 Agent、供应商与到期日、续费周期与支出汇总、聚合续费提醒、Telegram 通知、常用命令。
- 浏览器 SSH / SFTP：手机快捷键与粘贴、多标签、目录浏览、新建文件与目录、上传下载队列、分块续传、UTF-8 编辑和冲突检查。
- 终端、文件浏览与传输使用独立 SSH 连接；保活与重新协商分别计时。异常断线五分钟内可恢复原 Shell，只允许原登录恢复；主动断开、关闭或退出登录会结束会话，不自动重发命令。
- 二步验证、一次性恢复码、设备撤销、加密备份、S3 / WebDAV 异地保存与跨域名迁移。
- 五套主题和自定义主题包，支持背景图、配色、局部样式与站点图标。
- 签名更新、健康检查、失败恢复与手动回退。

## 全新服务器安装

先把域名 A 记录指向服务器；存在 AAAA 时也须指向本机 IPv6。关闭 CDN 代理，放行 TCP 80、443。

~~~sh
curl -fL --proto '=https' --proto-redir '=https' --tlsv1.2 https://raw.githubusercontent.com/coexacx/yuji-probe-rust/v0.10.1/install.sh -o /root/yuji-install.sh
bash /root/yuji-install.sh
~~~

脚本询问域名、站点名称、管理员用户名与密码，自动安装 Nginx、申请 HTTPS 证书并启用续期。适用于支持列表中的全新 Linux + systemd 主机；已有宝塔或网站请使用 [手动部署教程](docs/Nginx-Rust部署教程.md)。不要在已有安装上重跑全新安装器。

## 发行内容

主控 0.10.1，Agent 0.2.2。完整包为 **yuji-probe-rust-0.10.1.zip**，包含 amd64 / arm64 预编译主控与完整源码；Agent 二进制、签名清单及安装脚本在同一 Release。GitHub 自动生成的 Source code 包不含主控二进制。

更新只检查本仓库 v* 正式发行版。下载核对 Ed25519 签名、版本、架构、长度与 SHA-256，失败即停止。普通用户不需要 GitHub Token 或编译环境。

## 部署与安全

Nginx 接受 HTTPS；Rust 只监听回环地址。状态目录位于 Web 公开目录之外，运行服务使用独立系统用户。安装通过服务器私有所有权链接完成，安装包不包含预设管理员、真实节点或运行密钥。

本轮拆分保留现有协议、数据格式和发布验签公钥，已安装 Agent 无需重装。二进制名称和 Agent 服务名沿用原项目，避免破坏迁移兼容。

主控仍有一个上游 RSA 依赖告警；当前受管私钥仅使用 Ed25519，未调用受影响的 RSA 私钥操作。详见 [安全边界](docs/RUST-SECURITY.md)。扫描不能保证不存在未知漏洞。

[0.10.0 性能比较基线](docs/验收-0.10.0.md) · [0.10.1 仓库拆分与验收](docs/仓库迁移-0.10.1.md)。ARM64 交叉构建支持不等于 ARM 真机测试。

本项目从羽迹探针 0.10.0（提交 2458ec1）拆分，采用 MIT 许可证，保留第三方版权和许可证。源码及发行文件不包含签名私钥。
