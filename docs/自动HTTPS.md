# 自动配置 Nginx、HTTPS 与反向代理

适用于 Rust 独立版 0.11.1 起的一键安装实例。基础安装仍只要求站点名称，结束显示 IP:端口；自动 HTTPS 是安装后的可选功能。

## 使用

1. 将域名的 A 记录解析到主控服务器；有 AAAA 记录时也必须正确。Cloudflare 可以保持橙云代理。
2. 云安全组和本机防火墙允许访问 80、443。Cloudflare 的 SSL/TLS 回源模式应设为 **Full (strict)**。Flexible 会造成 HTTPS 跳转循环，脚本不会代改账户设置。
3. 运行 sudo yuji-probe，选择 **13 配置 Nginx + HTTPS**，填写域名，例如 probe.example.com。不要填写协议、端口或路径。
4. 通常无需其他输入。脚本申请 Let’s Encrypt 证书、生成站点、启用自动续期，并将主控改为回环监听。完成后显示 https://probe.example.com。

也可以直接运行：

~~~sh
sudo yuji-probe https
~~~

域名输入前会显示 Let’s Encrypt 订阅协议链接。脚本使用无邮箱 ACME 账户，不要求再填写邮箱；续期结果通过本机 systemd 日志查看。公开证书会进入证书透明度日志。

此功能面向 /opt/yuji-probe、yuji-probe.service 的一键安装实例。已有宝塔手动安装的自定义目录、用户和服务名继续使用 [手动反向代理文档](反向代理.md)，不要将一键菜单强行指向旧实例。

## Cloudflare 已开启代理

脚本不会以“域名 IP 与本机不同”为理由拒绝橙云域名。它先在本机创建随机验证文件，再通过公网域名检查 ACME 路径，成功后使用 HTTP-01 申请证书。

若 WAF、人机验证、回源限制或严格 HTTPS 引导阻断 HTTP 验证，菜单会提示输入 **Cloudflare API Token**，改用 DNS-01。整个过程不需要关闭橙云，也不修改 A / AAAA 记录、代理状态或 SSL/TLS 模式。

创建 Token：

1. 打开 https://dash.cloudflare.com/profile/api-tokens 。
2. 选择自定义 Token，只授予目标 Zone 的 **DNS: Edit** 和 **Zone: Read** 权限。
3. Zone Resources 限定为该域名所属的一个 Zone，例如 example.com，不要选择所有域名。
4. 将 Token 粘贴到终端的隐藏输入框。无需提供账户全局 API Key。

Token 只用于临时创建、删除 _acme-challenge 的 TXT 记录。凭据按域名存放于 /etc/yuji-probe/https/yuji-*-cloudflare.ini，权限 0600，仅 root 可读；不会进入命令行参数、源码包或面板数据库。续期仍需要这个 Token，撤销或限制失效后需及时更换。

**只填写域名无法绕过任意 Cloudflare 安全规则。** HTTP 验证受阻时需要上述受限 Token，或自行放行 /.well-known/acme-challenge/。没有 Token、也无法开放验证路径时会退出并恢复站点配置。证书签发成功也不代表 WAF 会允许 Agent WSS；正常业务仍需允许节点访问 /api/agent、浏览器访问 /api/terminal，以及面板 API。

非 Cloudflare DNS 且 HTTP 验证不可用时，请按手动文档使用对应 DNS 服务商的证书工具。

## 已有 Nginx 与新装 Nginx

- 先识别实际运行的 Nginx 主进程、二进制和启动参数，包括宝塔的 /www/server/nginx/sbin/nginx。
- 已安装时复用当前版本，不重新安装、不替换二进制，也不停止其他站点。
- 没有安装时，Debian 12/13、Ubuntu 22.04/24.04/26.04、Rocky / AlmaLinux 9/10、CentOS Stream 9/10 使用 nginx.org 官方稳定仓库的最新预编译包。下载校验 HTTPS、仓库签名和固定的官方 GPG 指纹，不在服务器编译 Nginx。
- nginx.org 没有 Fedora 专用包。Fedora 42/43/44 使用发行版官方仓库的最新可用二进制包，不混装 EL RPM。
- 支持 amd64、arm64；实际安装版本由对应仓库决定。已有 Nginx 需要 SSL 和 realip 模块。
- 新站点写入现有 http 区块引用的站点目录，文件名为 yuji-probe-managed.conf。不会覆盖主 nginx.conf、默认站点或其他域名文件。
- 同名域名、通配符站点、不能确定归属的正则 server_name、多个 Nginx 主进程、80/443 被其他程序占用时停止自动操作。自定义 Nginx 未提供 http 级别的 *.conf 或 * include 时使用手动配置。
- 同名站点已经在宝塔创建时，直接在该站点内配置代理即可。菜单不会自动接管宝塔已有的证书或站点文件。

SELinux 启用时，为主控端口安装单独的连接策略，并标记本工具自己的证书和 ACME 目录；不关闭 SELinux，不开启允许全部出站连接的全局布尔开关。若端口已有其他 SELinux 用途，会停止而不重新标记它。

## 证书与运行文件

证书工具使用 /opt/yuji-probe-certbot 中的独立 Python 虚拟环境，不修改系统 Python 包。先更新该环境内的 pip，再从官方 PyPI 安装兼容的 Certbot 和 Cloudflare 插件，只接收预编译 wheel。它只在申请、续期时运行，没有额外常驻 Python 服务。

| 内容 | 路径 |
| --- | --- |
| 站点归属与摘要 | /etc/yuji-probe/https/site.json |
| ACME 账户、证书、续期配置 | /etc/yuji-probe/https/letsencrypt |
| Cloudflare 凭据 | /etc/yuji-probe/https/yuji-*-cloudflare.ini |
| 公开验证文件目录 | /var/lib/yuji-probe-acme |
| 私有工具日志 | /var/lib/yuji-probe-https |
| 续期计时器 | yuji-probe-https-renew.timer |
| 续期服务 | yuji-probe-https-renew.service |

计时器每天检查两次，并有随机延迟；只有接近续期时间才向 CA 申请。发生实际续期后，先校验证书和 Nginx 配置，再平滑重载。证书未变化时不会为例行检查重载 Nginx。续期脚本随签名发行包更新，不依赖单独下载的远程脚本。

~~~sh
systemctl list-timers yuji-probe-https-renew.timer
systemctl start yuji-probe-https-renew.service
journalctl -u yuji-probe-https-renew.service -n 60 --no-pager
~~~

首次申请或续期失败的详情保存在 /var/lib/yuji-probe-https/last-certbot.log，仅 root 可读。Token 失效时使用 root 编辑对应凭据文件，保持 0600，再启动续期服务。不要公开原始证书日志、ACME 账户目录或凭据文件。

重复选择菜单 13、填写同一域名，会检查当前站点并保留有效证书，避免重复申请。自动生成文件被手动编辑后，摘要检查会阻止静默覆盖；此时请核对文件，使用手动管理或恢复本工具原配置。请勿修改 site.json 中的摘要来跳过检查。

## 失败、迁移与卸载

配置前检查已有站点，写入后先执行 Nginx 配置校验再重载。证书失败、HTTPS 健康检查失败或续期服务安装失败时，恢复原站点文件和主控入口；已下载的运行依赖、私有 ACME 账户及证书会保留，便于重试。不要在配置期间强制断电；操作系统崩溃、磁盘只读等情况不能依靠进程内回滚，需通过 SSH 检查现场。

已有节点且需要改变主控域名时，先使用后台备份与迁移流程同步 Agent 连接信息；菜单不会只修改地址而留下失联的节点。自动 HTTPS 不代替已有的面板跨域名迁移功能。

菜单 12 的明确卸载操作会检查归属、移除本工具未被修改的站点和续期计时器；保留面板数据、证书与凭据目录。其他 Nginx 站点、Nginx 本身和系统 Python 不会被卸载。证书工具环境与专用 SELinux 策略保留供核对，不删除他人的系统配置。若自动站点已被手动更改，卸载会要求先核对，避免删除不再属于原配置的内容。

## 官方资料

- [Let’s Encrypt 验证方式](https://letsencrypt.org/docs/challenge-types/)
- [Nginx 官方 Linux 软件包](https://nginx.org/en/linux_packages.html)
- [Certbot Cloudflare 插件与 Token 权限](https://certbot-dns-cloudflare.readthedocs.io/en/stable/)
- [Cloudflare Full (strict)](https://developers.cloudflare.com/ssl/origin-configuration/ssl-modes/full-strict/)
