# 宝塔 Nginx 配置位置与完整示例

适用于羽迹探针 Rust 独立版 0.10.1。先按 [部署教程](Nginx-Rust部署教程.md) 创建 Rust 服务、站点和 HTTPS 证书，再配置本页的反向代理。示例域名为 `probe.example.com`，内部监听为 `127.0.0.1:19282`；请替换为自己的实际值。

## 先说明此前教程与实际部署的区别

此前教程给出了完整的代理片段，却没有说明现有宝塔部署采用了两份文件。实际布局是：

| 宝塔操作位置 | 常见文件路径 | 本教程放置的内容 |
| --- | --- | --- |
| 网站 → 目标站点 → 配置文件 | `/www/server/panel/vhost/nginx/probe.example.com.conf` | 域名、证书、ACME 验证、站点参数，以及指向伪静态文件的 `include` |
| 网站 → 目标站点 → 伪静态 | `/www/server/panel/vhost/rewrite/probe.example.com.conf` | 敏感路径限制、WebSocket 和普通请求的 `location` 代理规则 |
| Rust systemd 服务 | `/etc/systemd/system/YJ.service` | `-listen 127.0.0.1:19282 -origin https://probe.example.com -web` |

文件名、界面名称以实际宝塔站点为准；核对主配置中的 `include` 路径，不要修改其他站点。

“伪静态”是宝塔的编辑入口名称。只要对应文件由 HTTPS `server { ... }` 中的 `include` 引入，文件里的合法指令就属于该配置上下文；其中的 `proxy_pass` 仍然执行反向代理，并不会变成 URL 重写。这与 Nginx 的 [include 机制](https://nginx.org/en/docs/ngx_core_module.html#include) 一致。

**方案一对应当前生产布局。方案二将内容集中到主配置。两种方式任选一种，不能把相同 `location` 同时放在两处。** 本次文档修正不要求已正常工作的站点更换布局。

## 配置前：保留哪些内容，移除哪些冲突

先备份本网站的主配置和伪静态文件到站点目录之外。以下路径替换为自己的站点；伪静态文件尚不存在时可跳过第二个 `cp`：

~~~sh
yuji_backup="/root/yuji-nginx-$(date +%Y%m%d-%H%M%S)"
install -d -m 700 "$yuji_backup"
cp /www/server/panel/vhost/nginx/probe.example.com.conf "$yuji_backup/site.conf"
cp /www/server/panel/vhost/rewrite/probe.example.com.conf "$yuji_backup/rewrite.conf"
~~~

保留本网站的 `listen`、`server_name`、SSL 证书路径、HTTPS 跳转、证书申请与续期验证配置、已有的可信代理 IP 配置。程序目录 `/opt/YJ` 和私有数据目录 `/var/lib/YJ` 不需要设为 Nginx 的网站根目录；页面和静态资源均由 Rust 返回。

仅在本网站移除以下冲突项，再按所选方案配置：

- `include enable-php-*.conf`、旧 PHP `fastcgi_pass`、将请求交给 `index.php` 的规则。
- 旧的 `location /`、`/api/agent`、`/api/terminal` 和 `/api/enroll/claim` 代理。新的每项只保留一份。
- 宝塔默认直接从本地目录读取 JS、CSS、图片的正则 `location`，例如 `location ~ .*.(js|css)?$`。本版资源嵌入 Rust，这些旧规则可能抢先匹配并导致资源 404。
- 同一层重复的六项站点参数。已有参数直接修改值，不再追加第二份。

本文采用手动配置。完成后无需再到宝塔“反向代理”入口添加一套同路径规则。不要修改全局 Nginx 配置或其他站点的 PHP 池。

## 方案一：主配置放站点参数，伪静态放代理规则

### 第一步：网站“配置文件”

在本网站已有的 **HTTPS `server { ... }` 内部**加入以下内容。证书、域名等内容继续使用宝塔当前配置。这里不另包 `http {}` 或 `server {}`，也不要放进 `location {}`。

~~~nginx
autoindex off;
client_max_body_size 24m;
client_header_timeout 10s;
client_body_timeout 10s;
send_timeout 15s;
keepalive_timeout 30s;
# 不将安装链接的查询参数写入访问日志；保留站点原有 error_log。
access_log off;

# 此 include 必须位于上面同一个 HTTPS server 块内，且只保留一次。
# 宝塔通常已经生成这一行；存在时直接沿用。
include /www/server/panel/vhost/rewrite/probe.example.com.conf;
~~~

如果站点将 80 和 443 分为两个 `server`，在 HTTPS 块中配置业务代理；80 端口继续负责 HTTPS 跳转与 ACME。如果宝塔采用一个同时监听 80/443 的块，保留它现有的 HTTPS 跳转和证书验证例外。

### 第二步：网站“伪静态”

选择手动填写，清除本网站旧的 PHP 重写规则，把下面**整段**放入伪静态编辑框。此处只放这些 `location`，不重复第一步参数、`include` 或外层 `server`。所有 `19282` 都要和 Rust 实际监听端口一致。

~~~nginx
# BEGIN YUJI LOCATIONS
location = /_internal/health { return 404; }
location ~ ^/(?:app|bin|storage|backend|src|source|docs|ops|qa|vendor|node_modules)(?:/|$) { return 404; }
location ~ /\. { return 404; }
location ~ \.php(?:/|$) { return 404; }

location ~ ^/api/(?:agent|terminal)$ {
    proxy_pass http://127.0.0.1:19282;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Probe-Gateway "";
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_read_timeout 185s;
    proxy_send_timeout 30s;
    proxy_buffering off;
}
location = /api/enroll/claim {
    proxy_pass http://127.0.0.1:19282;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Probe-Gateway "";
    proxy_set_header Connection "";
    proxy_read_timeout 185s;
    proxy_send_timeout 30s;
    proxy_buffering off;
}
location / {
    proxy_pass http://127.0.0.1:19282;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Probe-Gateway "";
    proxy_set_header Connection "";
    proxy_read_timeout 65s;
    proxy_send_timeout 30s;
}
# END YUJI LOCATIONS
~~~

### 第三步：保存与加载

已有站点切换布局时，建议在终端编辑好上述两份文件，再统一执行下文的检查和 reload。这样可避免宝塔分别保存时，暂时出现“旧规则仍在、新规则已加入”的重复 `location`。检查未通过时，先修正文件，不执行 reload。

## 方案二：所有内容集中放在主配置

使用仓库中的 [宝塔站点完整片段](../ops/templates/nginx-baota-rust.server.inc)。该文件包含六项站点参数、访问日志设置以及方案一的全部代理规则。

1. 保留站点现有域名、证书、HTTPS 跳转和 ACME 验证配置。
2. 将模板**全文**放在现有 HTTPS `server { ... }` 内部。已有的同名参数和代理规则先合并移除；模板没有外层 `server`，不能单独当作顶层虚拟主机配置。
3. 将本网站“伪静态”内容改为仅注释，例如 `# Rust 代理规则已放在站点主配置中。`。
4. 主配置原来的伪静态 `include` 行可以保留，但它指向的文件必须存在且只有注释或为空。不要在两份文件中重复代理规则。
5. 按下一节检查并 reload。

两种布局的业务规则相同，区别只是文件组织。**发布包内原有的 `ops/templates/nginx-rust.conf` 是全新服务器一键安装模板**，包含两个完整 `server` 块，使用 `19281` 和一键安装器的证书路径；不要直接将它粘贴进宝塔已经存在的 `server`。本页新增的 `nginx-baota-rust.server.inc` 专用于宝塔手动部署，默认 `19282`。

## 六项参数与 WebSocket 超时分别表示什么

| 配置 | 含义 |
| --- | --- |
| `autoindex off;` | 关闭 Nginx 本地目录列表；不影响登录后经 Agent 使用的 SFTP 文件浏览。 |
| `client_max_body_size 24m;` | 限制单次 HTTP 请求体大小；不等于 SSH/SFTP 分块传输文件的总大小限制。 |
| `client_header_timeout 10s;` | 接收客户端请求头的超时。 |
| `client_body_timeout 10s;` | 接收 HTTP 请求体时，两次读取之间的超时。 |
| `send_timeout 15s;` | 向客户端发送响应时，两次写入之间的超时。 |
| `keepalive_timeout 30s;` | 普通 HTTP 长连接等待下一次请求的空闲时间。 |

这些参数统一放在站点 `server` 层，便于维护。并不是六项只能写在“配置文件”编辑框；如果包含文件确实在 `server` 层加载，语法上下文仍相同。需要避免的是把 `client_header_timeout` 等指令误放进 `location`。参数含义见 Nginx [HTTP 核心模块](https://nginx.org/en/docs/http/ngx_http_core_module.html) 和 [autoindex 模块](https://nginx.org/en/docs/http/ngx_http_autoindex_module.html#autoindex)。

`keepalive_timeout 30s` 并不表示 SSH 会话会在 30 秒后被强制关闭。WebSocket 使用单独的升级请求头和 `proxy_read_timeout 185s`；185 秒限制的是两次从上游读取数据之间的间隔，心跳也可以维持连接，不能将它理解为会话总时长。`proxy_send_timeout 30s` 处理向上游写入的停滞。参见 Nginx 的 [WebSocket 代理说明](https://nginx.org/en/docs/http/websocket.html) 与 [代理读取超时](https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_read_timeout)。

## 检查、加载与访问验证

宝塔常用 Nginx 路径如下；如果自己的安装路径不同，使用该实例实际运行的 Nginx：

~~~sh
/www/server/nginx/sbin/nginx -t
~~~

只有输出检查成功，才执行：

~~~sh
/www/server/nginx/sbin/nginx -s reload
~~~

`reload` 重新加载配置；通常无需停止 Nginx 或重启 Rust。需要确认文件是否被加载时，在服务器本地查看 `/www/server/nginx/sbin/nginx -T` 输出中的配置文件路径和对应 `include`。不要只检查另一个未运行的系统 Nginx。

~~~sh
systemctl status YJ --no-pager
ss -ltnp 'sport = :19282'
curl -I https://probe.example.com/
curl -i https://probe.example.com/_internal/health
curl -i https://probe.example.com/storage/control/auth.json
~~~

预期：Rust 监听 `127.0.0.1:19282`；首页返回 200；后两条敏感路径返回 404。浏览器页面应正常加载 JS/CSS，管理员登录后可打开 SSH 和文件浏览，节点保持在线。浏览器开发者工具中，终端 WebSocket 升级返回 101。

`-origin` 必须与公网 HTTPS 地址一致；手动教程默认 443。若使用非标准 HTTPS 端口，`-origin` 带上端口，并将各代理中的 `proxy_set_header Host $host` 改为 `proxy_set_header Host $http_host`。内部 `proxy_pass` 端口仍然是 Rust 的回环监听端口。

`X-Real-IP` 由 Nginx 按连接来源覆盖，`X-Probe-Gateway` 清空。使用 CDN 时，先在自己的 Nginx 中配置可信代理地址，再保留本页的覆盖行为。普通用户从公网使用 HTTPS/WSS；Nginx 与同机 Rust 通过回环 HTTP/WS 通信，云防火墙无需开放 19282。

## 常见错误与恢复

| 现象 | 核对位置 |
| --- | --- |
| `duplicate location` | 主配置、伪静态文件和其他被包含文件是否重复写了同一路由。 |
| `directive is not allowed here` | 是否把服务器参数放进 `location`，或把整套 `server` 再嵌套进站点 `server`。 |
| 重复指令错误 | 原有参数应修改值，不要在相同层级重复追加。 |
| 首页打开但 JS/CSS/图片 404 | 是否残留优先匹配本地文件的静态资源 `location`；检查代理端口和 Rust 是否启动。 |
| 502 | Rust 服务状态、回环地址和 `proxy_pass` 端口是否一致。 |
| 421 | 域名、Host 头、HTTPS 端口与 Rust `-origin` 是否一致。 |
| 网页正常但终端连不上 | WebSocket 升级头是否完整，是否被其他 `location` 匹配；在面板检查 Agent 在线状态和版本。 |
| SSL 续期验证失败 | 是否保留了原站点的 ACME 验证目录和 `/.well-known/acme-challenge/` 配置。 |

如需撤销本次配置编辑，把备份的主配置与伪静态文件**一起**恢复到原路径，执行 `nginx -t` 成功后再 reload。只恢复其中一个文件可能留下重复或缺失规则。

本页与新增模板属于部署文档修正，不改变 Rust 0.10.1 主控二进制和发布包签名；下载过旧版包内教程的用户，以本仓库最新文档为准。
