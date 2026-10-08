#!/usr/bin/env python3
"""Install YJ and the legacy alias without overwriting unrelated commands."""
import argparse,os,pathlib,stat,sys,tempfile

def content(root):
    # The existing management menu is for the standard one-click instance.
    if pathlib.Path(root) not in (pathlib.Path("/opt/YJ"),pathlib.Path("/opt/yuji-probe")):
        raise ValueError("此菜单只用于 /opt/YJ 或兼容的旧版一键安装实例；手动实例继续使用已注册的更新器")
    return '#!/bin/sh\nexec /bin/bash '+str(pathlib.Path(root))+'/manage.sh "$@"\n'

def paths(directory,root="/opt/yuji-probe"):
    names=("YJ",) if pathlib.Path(root)==pathlib.Path("/opt/YJ") else ("YJ","yuji-probe")
    return [pathlib.Path(directory)/name for name in names]

def check(path,expected):
    if not path.exists() and not path.is_symlink():return
    st=path.lstat()
    if not stat.S_ISREG(st.st_mode) or st.st_uid!=0 or st.st_mode&0o022 or path.read_text()!=expected:
        raise ValueError("已有命令不属于本程序，未覆盖："+str(path))

def install(root,directory="/usr/local/bin"):
    expected=content(root);directory=pathlib.Path(directory)
    for parent in [*reversed(directory.parents),directory]:
        st=parent.lstat()
        if not stat.S_ISDIR(st.st_mode) or st.st_uid!=0 or st.st_mode&0o022:
            raise ValueError("命令目录权限不正确")
    for path in paths(directory,root):check(path,expected)
    for path in paths(directory,root):
        fd,name=tempfile.mkstemp(prefix=".yj-",dir=directory)
        try:
            with os.fdopen(fd,"w") as f:
                os.fchmod(f.fileno(),0o755);f.write(expected);f.flush();os.fsync(f.fileno())
            os.replace(name,path)
        finally:
            if os.path.exists(name):os.unlink(name)

def remove(root,directory="/usr/local/bin"):
    expected=content(root)
    for path in paths(directory,root):
        try:check(path,expected)
        except ValueError:continue
        path.unlink(missing_ok=True)

if __name__=="__main__":
    try:
        if os.geteuid()!=0:raise ValueError("请使用 root 运行")
        parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--root",default=str(pathlib.Path(__file__).resolve().parent.parent))
        args=parser.parse_args();install(args.root);print("管理菜单：sudo YJ")
    except Exception as e:print(str(e),file=sys.stderr);sys.exit(1)
