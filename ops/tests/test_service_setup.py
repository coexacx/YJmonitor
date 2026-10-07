import importlib.util,os,pathlib,stat,tempfile,types,unittest
from unittest.mock import patch
OPS=pathlib.Path(__file__).resolve().parents[1]
def module(name,file):
 spec=importlib.util.spec_from_file_location(name,OPS/file);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
prepare=module("prepare_service","prepare-service.py");menu=module("install_command","install-command.py")
@unittest.skipUnless(os.geteuid()==0,"ownership tests require an isolated root runner")
class ServiceSetup(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory(prefix="yuji-setup-test-",dir="/srv");self.base=pathlib.Path(self.tmp.name)
  self.root=self.base/"program";(self.root/"bin").mkdir(parents=True)
  for arch in ("amd64","arm64"):(self.root/"bin"/("probe-linux-"+arch)).write_bytes(b"fixture")
  self.data=self.base/"data";self.user="yuji-setup-fixture"
  self.account=types.SimpleNamespace(pw_uid=60000,pw_gid=60000,pw_dir=str(self.data),pw_shell="/usr/sbin/nologin")
  self.pw=patch.object(prepare.pwd,"getpwnam",return_value=self.account);self.pw.start()
  self.gr=patch.object(prepare.grp,"getgrgid",return_value=types.SimpleNamespace(gr_name=self.user));self.gr.start()
  self.run=patch.object(prepare.subprocess,"run",side_effect=AssertionError("unexpected useradd"));self.run.start()
 def tearDown(self):
  self.run.stop();self.gr.stop();self.pw.stop();self.tmp.cleanup()
 def run_prepare(self):return prepare.prepare(self.root,self.data,self.user)
 def test_private_directory_and_repeat_preserve_data(self):
  self.assertEqual(self.run_prepare(),60000)
  saved=self.data/"control"/"existing.json";saved.write_text("unchanged")
  self.run_prepare();self.assertEqual(saved.read_text(),"unchanged")
  for p in (self.data,self.data/"control"):
   self.assertEqual(p.stat().st_uid,60000);self.assertEqual(stat.S_IMODE(p.stat().st_mode),0o700)
  self.assertEqual(stat.S_IMODE((self.root/"bin/probe-linux-amd64").stat().st_mode),0o755)
 def test_new_account_uses_system_nologin_and_no_shell(self):
  with patch.object(prepare.pwd,"getpwnam",side_effect=[KeyError(),self.account]),patch.object(prepare.subprocess,"run") as run:
   self.run_prepare()
   self.assertEqual(run.call_args.args[0],["/usr/sbin/useradd","--system","--user-group","--home-dir",str(self.data),"--shell","/usr/sbin/nologin",self.user])
 def test_existing_foreign_data_is_not_claimed(self):
  self.data.mkdir();(self.data/"keep").write_text("keep")
  with patch.object(prepare.pwd,"getpwnam",side_effect=KeyError()),self.assertRaises(ValueError):self.run_prepare()
  self.assertEqual((self.data/"keep").read_text(),"keep")
 def test_data_symlink_is_rejected(self):
  self.data.symlink_to(self.root,target_is_directory=True)
  with self.assertRaises(ValueError):self.run_prepare()
 def test_program_symlink_is_rejected(self):
  p=self.root/"bin/probe-linux-amd64";p.unlink();p.symlink_to("/bin/true")
  with self.assertRaises(ValueError):self.run_prepare()
 def test_world_writable_parent_is_rejected(self):
  self.base.chmod(0o777)
  with self.assertRaises(ValueError):self.run_prepare()
  self.base.chmod(0o700)
 def test_privileged_or_mismatched_account_is_rejected(self):
  for field,value in (("pw_uid",0),("pw_dir","/root"),("pw_shell","/bin/bash")):
   original=getattr(self.account,field);setattr(self.account,field,value)
   with self.assertRaises(ValueError):self.run_prepare()
   setattr(self.account,field,original)
  self.assertFalse(self.data.exists())
 def test_data_inside_program_and_invalid_username_are_rejected(self):
  with self.assertRaises(ValueError):prepare.prepare(self.root,self.root/"storage",self.user)
  with self.assertRaises(ValueError):prepare.prepare(self.root,self.data,"--root")
 def test_permissive_existing_directory_is_rejected(self):
  self.run_prepare();self.data.chmod(0o755)
  with self.assertRaises(ValueError):self.run_prepare()

@unittest.skipUnless(os.geteuid()==0,"command directory ownership requires root")
class CommandSetup(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory(prefix="yj-menu-test-",dir="/srv");self.directory=pathlib.Path(self.tmp.name)
 def tearDown(self):self.tmp.cleanup()
 def install(self):menu.install("/opt/yuji-probe",self.directory)
 def test_installs_new_and_legacy_commands_idempotently(self):
  self.install();self.install()
  for p in menu.paths(self.directory):
   self.assertEqual(p.read_text(),menu.content("/opt/yuji-probe"));self.assertEqual(stat.S_IMODE(p.stat().st_mode),0o755)
 def test_foreign_command_blocks_both_writes(self):
  (self.directory/"YJ").write_text("foreign")
  with self.assertRaises(ValueError):self.install()
  self.assertEqual((self.directory/"YJ").read_text(),"foreign");self.assertFalse((self.directory/"yuji-probe").exists())
 def test_symbolic_link_is_not_followed(self):
  (self.directory/"YJ").symlink_to(self.directory/"missing")
  with self.assertRaises(ValueError):self.install()
  self.assertFalse((self.directory/"missing").exists())
 def test_remove_leaves_changed_foreign_command(self):
  self.install();(self.directory/"YJ").write_text("different command");menu.remove("/opt/yuji-probe",self.directory)
  self.assertTrue((self.directory/"YJ").exists());self.assertFalse((self.directory/"yuji-probe").exists())
 def test_refuses_other_instance_root(self):
  with self.assertRaises(ValueError):menu.install("/opt/other",self.directory)
if __name__=="__main__":unittest.main()
