#!/usr/bin/env python3
"""Root-only management for the one-click installation. No shell-evaluated configuration."""
import fcntl,importlib.util,ipaddress,json,os,pathlib,re,shutil,subprocess,sys,tarfile,tempfile,time,urllib.parse,urllib.request
ROOT=pathlib.Path('/opt/yuji-probe')
STATE=pathlib.Path('/var/lib/yuji-probe/control')
CONF=pathlib.Path('/etc/yuji-probe')
SERVICE='yuji-probe.service'
ARCH='arm64' if os.uname().machine=='aarch64' else 'amd64'
def command(*args,**kw):return subprocess.run(list(args),check=True,**kw)
def active():return subprocess.run(['systemctl','is-active','--quiet',SERVICE]).returncode==0
def trusted(path):
 info=path.lstat()
 if path.is_symlink() or info.st_uid!=0 or info.st_mode&0o022:raise ValueError('管理配置权限不正确')
 return json.loads(path.read_text())
def settings():return trusted(CONF/'instance.json')
def atomic(path,data):
 fd,name=tempfile.mkstemp(prefix='.manage-',dir=path.parent)
 try:
  with os.fdopen(fd,'w') as f:os.fchmod(f.fileno(),0o600);f.write(data);f.flush();os.fsync(f.fileno())
  os.replace(name,path)
 finally:
  if os.path.exists(name):os.unlink(name)
def url(value):
 u=urllib.parse.urlsplit(value)
 if u.scheme not in ('http','https') or not u.hostname or u.username or u.password or u.query or u.fragment or u.path not in ('','/') or any(c.isspace() or c in '%\\"' for c in value):raise ValueError('请填写 HTTP(S) 地址，不含路径或查询参数')
 if u.port is not None and not 1<=u.port<=65535:raise ValueError('端口不正确')
 if u.scheme=='http':
  ipaddress.ip_address(u.hostname)
  if u.port is None:raise ValueError('HTTP 入口必须填写 IP 和端口')
 host=u.hostname.encode('idna').decode().lower()
 if ':' in host:host='['+host+']'
 elif not re.fullmatch(r'[a-z0-9](?:[a-z0-9.-]{0,251}[a-z0-9])?',host):raise ValueError('主机名格式不正确')
 port='' if u.port is None or u.port==({'http':80,'https':443}[u.scheme]) else ':'+str(u.port)
 return u.scheme+'://'+host+port
def helper():
 spec=importlib.util.spec_from_file_location('updater',ROOT/'ops/update-panel.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
def health(origin,port):
 helper().check({'state':str(STATE),'origin':origin,'listen':'127.0.0.1:'+str(port)},None)
def configure(origin,port):
 origin=url(origin);port=int(port)
 if not 1024<=port<=65535:raise ValueError('端口范围为 1024–65535')
 u=urllib.parse.urlsplit(origin)
 if u.scheme=='http' and u.port!=port:raise ValueError('HTTP 地址中的端口需要与服务端口一致')
 listen=('0.0.0.0' if u.scheme=='http' else '127.0.0.1')+':'+str(port)
 if u.scheme=='http' and ':' in u.hostname:listen='[::]:'+str(port)
 previous=(CONF/'instance.json').read_text() if (CONF/'instance.json').exists() else None
 unit=pathlib.Path('/etc/systemd/system')/SERVICE
 old_unit=unit.read_text() if unit.exists() else None
 template=(ROOT/'ops/templates/yuji-probe-direct.service').read_text()
 data={'origin':origin,'port':port,'listen':listen}
 try:
  text=template.replace('__ARCH__',ARCH).replace('__ORIGIN__',origin).replace('__LISTEN__',listen)
  unit.write_text(text);unit.chmod(0o644);atomic(CONF/'instance.json',json.dumps(data))
  command('systemctl','daemon-reload');command('systemctl','enable',SERVICE,stdout=subprocess.DEVNULL)
  command('systemctl','restart',SERVICE);health(origin,port)
  command('python3',str(ROOT/'ops/update-panel.py'),'--configure','--name','main','--root',str(ROOT),'--state',str(STATE),'--service',SERVICE,'--origin',origin,'--listen','127.0.0.1:'+str(port),'--distribution','rust')
 except Exception:
  if old_unit is not None:
   unit.write_text(old_unit);atomic(CONF/'instance.json',previous)
   command('systemctl','daemon-reload');command('systemctl','restart',SERVICE)
  else:subprocess.run(['systemctl','disable','--now',SERVICE],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  raise
def summary():
 c=settings();version=subprocess.check_output([str(ROOT/'bin'/('probe-linux-'+ARCH)),'--version'],text=True).split()[1]
 print('  版本  '+version+'    状态  '+('运行中' if active() else '已停止'))
 print('  地址  '+c['origin'])
def update(rollback=False):
 m=helper();cfg=trusted(pathlib.Path('/etc/yuji-probe-rust-updaters/main.json'));request={'at':int(time.time()),'action':'rollback' if rollback else 'update'}
 if not rollback:
  with urllib.request.urlopen(urllib.request.Request('https://api.github.com/repos/coexacx/yuji-probe-rust/releases/latest',headers={'User-Agent':'Yuji-Probe-Manager'}),timeout=15) as r:data=json.loads(r.read(131072))
  version=data.get('tag_name','').removeprefix('v')
  if not re.fullmatch(r'\d{1,5}\.\d{1,5}\.\d{1,5}',version):raise ValueError('版本信息无效')
  current=subprocess.check_output([str(ROOT/'bin'/('probe-linux-'+ARCH)),'--version'],text=True).split()[1]
  if tuple(map(int,version.split('.')))<=tuple(map(int,current.split('.'))):print('\n  当前已是最新版本：'+current);return
  print('\n  '+current+' → '+version+'\n  更新会短暂重启主控，节点配置会保留。')
  if input('  确认更新？[y/N]：').lower()!='y':return
  request['version']=version
 else:
  if input('  回退会恢复上次升级前的数据。输入 ROLLBACK 确认：')!='ROLLBACK':return
 work=m.BASE/cfg['name'];m.private(work)
 with (work/'lock').open('a') as lock:
  fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB);version=m.apply(cfg,request)
 print('\n  已完成，版本 '+version)
def backup():
 m=helper();destination=pathlib.Path('/var/backups/yuji-probe');destination.mkdir(mode=0o700,parents=True,exist_ok=True)
 target=destination/('yuji-probe-'+time.strftime('%Y%m%d-%H%M%S')+'.tar.gz');running=active()
 try:
  if running:command('systemctl','stop',SERVICE)
  with tempfile.TemporaryDirectory(prefix='.snapshot-',dir=destination) as folder:
   work=pathlib.Path(folder);m.safe_snapshot(STATE.parent,work/'state',STATE.stat().st_uid);shutil.copytree(CONF,work/'config')
   shutil.copy2('/etc/systemd/system/'+SERVICE,work/SERVICE)
   with tarfile.open(target,'x:gz') as tar:
    for entry in work.iterdir():tar.add(entry,arcname=entry.name,recursive=True)
   target.chmod(0o600)
 finally:
  if running:command('systemctl','start',SERVICE)
 print('\n  备份已保存：'+str(target))
def address():
 c=settings();print('\n  当前地址  '+c['origin']+'\n  HTTPS 模式请先按文档配好反向代理。\n')
 value=input('  新的面板地址：').strip()
 if not value:return
 value=url(value)
 if json.loads((STATE/'nodes.json').read_text()).get('nodes',[]):
  raise ValueError('已有节点请使用后台面板迁移流程同步 Agent 地址，此菜单用于空面板首次设置')
 port=input('  服务端口 ['+str(c['port'])+']：').strip() or str(c['port'])
 configure(value,int(port));print('\n  已保存：'+value)
def reset_mfa():
 c=settings();running=active()
 try:
  if running:command('systemctl','stop',SERVICE)
  command(str(ROOT/'bin'/('probe-linux-'+ARCH)),'-state',str(STATE),'-origin',c['origin'],'-web','--reset-mfa')
 finally:
  if running:command('systemctl','start',SERVICE)
def uninstall():
 print('\n  卸载程序及管理服务，保留 /var/lib/yuji-probe 和 /etc/yuji-probe。')
 if input('  输入 UNINSTALL 确认：')!='UNINSTALL':return
 for service in [SERVICE,'yuji-probe-rust-update-main.path','yuji-probe-rust-update-main.service']:
  subprocess.run(['systemctl','disable','--now',service],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  (pathlib.Path('/etc/systemd/system')/service).unlink(missing_ok=True)
 pathlib.Path('/etc/yuji-probe-rust-updaters/main.json').unlink(missing_ok=True)
 pathlib.Path('/usr/local/bin/yuji-probe').unlink(missing_ok=True)
 shutil.rmtree(ROOT);command('systemctl','daemon-reload');print('\n  程序已卸载，面板数据已保留。')
def main():
 if os.geteuid()!=0:raise ValueError('请使用 root 运行')
 os.umask(0o077);action=sys.argv[1] if len(sys.argv)>1 else 'summary'
 with open('/run/yuji-probe-manage.lock','a') as lock:
  fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
  if action=='configure' and len(sys.argv)==4:configure(sys.argv[2],sys.argv[3])
  elif action=='summary':summary()
  elif action in ('start','stop','restart'):command('systemctl',action,SERVICE);summary()
  elif action=='update':update()
  elif action=='rollback':update(True)
  elif action=='backup':backup()
  elif action=='address':address()
  elif action=='reset-mfa':reset_mfa()
  elif action=='uninstall':uninstall()
  else:raise ValueError('可用命令：summary/start/stop/restart/update/rollback/backup/address/reset-mfa/uninstall')
if __name__=='__main__':
 try:main()
 except (Exception,KeyboardInterrupt) as e:print('\n  操作未完成：'+str(e),file=sys.stderr);sys.exit(1)
