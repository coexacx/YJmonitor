#!/usr/bin/env python3
"""Prepare a least-privilege service account and private state; never reset state."""
import argparse,fcntl,grp,os,pathlib,pwd,re,stat,subprocess,sys

def clean_path(value):
    path=pathlib.Path(value)
    if not path.is_absolute() or not re.fullmatch(r"/[A-Za-z0-9_./-]+",str(path)) or ".." in path.parts:
        raise ValueError("目录必须为不含空格或上级跳转的绝对路径")
    return path

def trusted_parents(path, include=True):
    chain=[*reversed(path.parents),path] if include else list(reversed(path.parents))
    for item in chain:
        info=item.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid!=0 or info.st_mode&0o022:
            raise ValueError("目录必须由 root 管理，且不可由其他用户写入："+str(item))

def prepare(root,data,user):
    root=clean_path(root);data=clean_path(data)
    if not re.fullmatch(r"[a-z_][a-z0-9_-]{0,30}",user):raise ValueError("服务用户名无效")
    if data==root or root in data.parents or data in root.parents:raise ValueError("程序与数据目录必须独立")
    trusted_parents(root);trusted_parents(data,False)
    binaries=[root/"bin"/("probe-linux-"+arch) for arch in ("amd64","arm64")]
    trusted_parents(root/"bin")
    for binary in binaries:
        info=binary.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_uid!=0 or info.st_mode&0o022:
            raise ValueError("程序文件所有者或权限不正确")
    try:account=pwd.getpwnam(user)
    except KeyError:account=None
    if account is None:
        if data.exists() or data.is_symlink():raise ValueError("数据目录已存在，请核对原部署，脚本不会接管已有数据")
        subprocess.run(["/usr/sbin/useradd","--system","--user-group","--home-dir",str(data),"--shell","/usr/sbin/nologin",user],check=True)
        account=pwd.getpwnam(user)
    group=grp.getgrgid(account.pw_gid)
    if account.pw_uid==0 or group.gr_name!=user or account.pw_dir!=str(data) or account.pw_shell not in ("/usr/sbin/nologin","/sbin/nologin","/bin/false"):
        raise ValueError("已有服务账户不符合预期，未修改账户或目录")
    for path in (data,data/"control"):
        try:
            path.mkdir(mode=0o700);os.chown(path,account.pw_uid,account.pw_gid)
        except FileExistsError:
            info=path.lstat()
            if not stat.S_ISDIR(info.st_mode) or info.st_uid!=account.pw_uid or info.st_gid!=account.pw_gid or info.st_mode&0o077:
                raise ValueError("已有数据目录所有者或权限不正确："+str(path))
    for binary in binaries:binary.chmod(0o755)
    return account.pw_uid

def main():
    if os.geteuid()!=0:raise ValueError("请使用 root 运行")
    os.umask(0o077)
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root",default=str(pathlib.Path(__file__).resolve().parent.parent))
    parser.add_argument("--data",default="/var/lib/yuji-probe-rust")
    parser.add_argument("--user",default="yuji-probe-rust")
    args=parser.parse_args()
    fd=os.open("/run/yuji-probe-prepare.lock",os.O_CREAT|os.O_RDWR|os.O_NOFOLLOW,0o600)
    with os.fdopen(fd,"w") as lock:
        fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        prepare(args.root,args.data,args.user)
    print("服务账户、私有数据目录和权限已准备完成。现有数据保持不变。")
if __name__=="__main__":
    try:main()
    except (Exception,KeyboardInterrupt) as e:print("准备失败："+str(e),file=sys.stderr);sys.exit(1)
