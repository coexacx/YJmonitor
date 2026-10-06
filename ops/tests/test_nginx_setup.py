"""Root-only deployment safety tests; no service or package mutations."""
import contextlib, hashlib, json, os, secrets, subprocess, sys, tempfile, unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import nginx_setup as n

class SafetyTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='yuji-https-test-',dir='/run')
        self.root=Path(self.temp.name);self.patches=[]
        for name,path in [('CONF',self.root/'etc'/'https'),('WORK',self.root/'work'),('WEBROOT',self.root/'web'),('VENV',self.root/'venv'),('SYSTEMD',self.root/'systemd')]:
            p=patch.object(n,name,path);p.start();self.patches.append(p)
        (self.root/'etc').mkdir();(self.root/'systemd').mkdir();n.prepare()
        self.site_dir=self.root/'sites';self.site_dir.mkdir()
        self.nginx={'binary':'/usr/sbin/nginx','args':[],'config':str(self.root/'nginx.conf'),'prefix':str(self.root)}
        self.state=self.root/'state';self.state.mkdir();(self.state/'nodes.json').write_text('{"nodes":[]}')
        self.settings={'origin':'http://203.0.113.5:19281','port':19281,'listen':'0.0.0.0:19281'}
    def tearDown(self):
        for p in reversed(self.patches):p.stop()
        self.temp.cleanup()
    def test_domain_normalization(self):
        self.assertEqual(n.domain_name(' Probe.Example.COM '),'probe.example.com')
        self.assertTrue(n.domain_name('探针.example.com').startswith('xn--'))
    def test_domain_injection_rejected(self):
        for value in ['x.com;return 200;','x.com\nserver{}','https://x.com','*.x.com','127.0.0.1','x.com:443','x.com/../a','-x.com','x..com','x.com.','x.com$(id)','x.com'+chr(96)+'id'+chr(96),'x.com#abc','localhost','a'*64+'.com']:
            with self.subTest(value=value),self.assertRaises(ValueError):n.domain_name(value)
    def test_cloudflare_edge_resolution_is_accepted(self):
        with patch.object(n.socket,'getaddrinfo',return_value=[(2,1,6,'',('104.16.1.5',80))]):
            self.assertEqual(n.resolved('probe.example.com'),{'104.16.1.5'})
    def test_private_dns_rejected(self):
        for ip in ['127.0.0.1','10.0.0.3','169.254.169.254','::1','192.168.1.2']:
            with patch.object(n.socket,'getaddrinfo',return_value=[(2,1,6,'',(ip,80))]),self.assertRaises(ValueError):n.resolved('probe.example.com')
    def test_write_rejects_symlink(self):
        target=self.root/'secret';target.write_text('original');link=self.root/'link';link.symlink_to(target)
        with self.assertRaises(ValueError):n.write(link,'changed')
        self.assertEqual(target.read_text(),'original')
    def test_write_rejects_writable_parent(self):
        path=self.root/'shared';path.mkdir();path.chmod(0o777)
        with self.assertRaises(ValueError):n.write(path/'config','data')
    def test_secret_mode_and_replacement(self):
        file=self.root/'config';n.write(file,'old');n.write(file,'new')
        self.assertEqual(file.stat().st_mode&0o777,0o600);self.assertEqual(file.read_text(),'new')
    def test_transaction_restores_bytes_and_modes(self):
        old=self.root/'old';old.write_text('original');old.chmod(0o640);new=self.root/'new';tx=n.Transaction()
        tx.put(old,'changed',0o644);tx.put(new,'added');tx.put(old,'again');tx.rollback()
        self.assertEqual(old.read_text(),'original');self.assertEqual(old.stat().st_mode&0o777,0o640);self.assertFalse(new.exists())
    def dump(self,name='existing.example.com'):
        return '# configuration file '+self.nginx['config']+':\nhttp { include '+str(self.site_dir)+'/*.conf; }\n# configuration file '+str(self.site_dir/'foreign.conf')+':\nserver { listen 80; server_name '+name+'; }\n'
    def test_foreign_exact_wildcard_regex_refused(self):
        for name in ['probe.example.com','*.example.com','.example.com','~^probe']:
            with patch.object(n,'nginx_check',return_value=self.dump(name)),self.assertRaises(ValueError):n.choose_site(self.nginx,'probe.example.com',None)
    def test_independent_site_selected(self):
        with patch.object(n,'nginx_check',return_value=self.dump()):self.assertEqual(n.choose_site(self.nginx,'probe.example.com',None),self.site_dir/n.SITE_NAME)
    def test_manual_edit_hash_blocks_overwrite(self):
        site=self.site_dir/n.SITE_NAME;site.write_text('user configuration')
        with patch.object(n,'nginx_check',return_value=self.dump()),self.assertRaises(ValueError):n.choose_site(self.nginx,'probe.example.com',{'site':str(site),'site_sha256':'0'*64})
        self.assertEqual(site.read_text(),'user configuration')
    def test_foreign_filename_refused(self):
        (self.site_dir/n.SITE_NAME).write_text('foreign')
        with patch.object(n,'nginx_check',return_value=self.dump()),self.assertRaises(ValueError):n.choose_site(self.nginx,'probe.example.com',None)
    def test_nested_include_domain_detected(self):
        extra=self.root/'names.inc';dump=self.dump().replace('server_name existing.example.com;','include '+str(extra)+';')
        dump+='# configuration file '+str(extra)+':\nserver_name probe.example.com;\n'
        with patch.object(n,'nginx_check',return_value=dump),self.assertRaises(ValueError):n.choose_site(self.nginx,'probe.example.com',None)
    def test_stream_is_not_http(self):
        dump='# configuration file '+self.nginx['config']+':\nstream { include '+str(self.site_dir)+'/*.conf; }\n'
        with patch.object(n,'nginx_check',return_value=dump),self.assertRaises(ValueError):n.choose_site(self.nginx,'probe.example.com',None)
    def test_baota_arguments(self):
        self.assertEqual(n.nginx_args('/www/server/nginx/sbin/nginx -c /www/server/nginx/conf/nginx.conf -p /www/server/nginx'),['-c','/www/server/nginx/conf/nginx.conf','-p','/www/server/nginx'])
        self.assertEqual(n.nginx_args('/usr/sbin/nginx -g daemon on; master_process on;'),['-g','daemon on; master_process on;'])
    def test_redirect_cannot_escape_domain(self):
        for url in ['https://evil.example/a','http://127.0.0.1/a','file:///etc/passwd','https://probe.example.com:8080/a']:
            with self.subTest(url=url),self.assertRaises(ValueError):n.SameHostRedirect('probe.example.com').redirect_request(None,None,301,'',{},url)
    def test_proxy_security_and_streaming(self):
        text=n.render('probe.example.com',19281,True,True)
        for item in ['listen [::]:443 ssl','proxy_read_timeout 3600s','proxy_request_buffering off','proxy_set_header X-Probe-Gateway ""','proxy_set_header X-Real-IP $remote_addr','location = /_internal/health { return 404; }','ssl_protocols TLSv1.2 TLSv1.3']:self.assertIn(item,text)
        self.assertNotIn('set_real_ip_from 0.0.0.0/0',text);self.assertNotIn('storage',text)
    def test_invalid_proxy_ports_rejected(self):
        for value in [True,0,443,65536,'19281;']:
            with self.assertRaises(ValueError):n.render('probe.example.com',value,True,False)
    def test_cloudflare_token_private_not_in_command(self):
        token=secrets.token_urlsafe(30)
        with patch.object(n.getpass,'getpass',return_value=token):file=n.cloudflare_credentials('probe.example.com')
        self.assertEqual(file.stat().st_mode&0o777,0o600);self.assertIn(token,file.read_text())
        with patch.object(n,'http_preflight',return_value=False),patch.object(n,'certbot_run') as runner:self.assertEqual(n.issue('probe.example.com',Path('/usr/bin/certbot')),'cloudflare')
        args=runner.call_args[0][0];self.assertNotIn(token,' '.join(args));self.assertIn('--dns-cloudflare',args)
    def test_bad_token_cannot_inject_ini(self):
        with patch.object(n.getpass,'getpass',return_value='abc\nother_setting=x'),self.assertRaises(ValueError):n.cloudflare_credentials('probe.example.com')
    def test_http_success_no_token_prompt(self):
        with patch.object(n,'http_preflight',return_value=True),patch.object(n,'certbot_run'),patch.object(n,'cloudflare_credentials') as credentials:
            self.assertEqual(n.issue('probe.example.com',Path('/usr/bin/certbot')),'http');credentials.assert_not_called()
    def test_http_failure_falls_back_to_dns(self):
        with patch.object(n,'http_preflight',return_value=True),patch.object(n,'certbot_run',side_effect=[ValueError('failed'),None]) as run,patch.object(n,'cloudflare_credentials',return_value=n.CONF/'test.ini'):
            self.assertEqual(n.issue('probe.example.com',Path('/usr/bin/certbot')),'cloudflare');self.assertEqual(run.call_count,2)
    def test_existing_dns_method_not_downgraded(self):
        with patch.object(n,'http_preflight') as preflight,patch.object(n,'certbot_run'),patch.object(n,'cloudflare_credentials',return_value=n.CONF/'test.ini'):
            self.assertEqual(n.issue('probe.example.com',Path('/usr/bin/certbot'),'cloudflare'),'cloudflare');preflight.assert_not_called()
    def test_nodes_block_origin_change_before_mutation(self):
        (self.state/'nodes.json').write_text('{"nodes":[{"id":"test"}]}')
        with patch('builtins.input',return_value='probe.example.com'),patch.object(n,'prepare') as prepare,self.assertRaises(ValueError):n.setup(self.settings,lambda *_:None,self.state)
        prepare.assert_not_called()
    def setup_context(self):
        stack=contextlib.ExitStack()
        for name,value in [('resolved',{'1.1.1.1'}),('discover_nginx',self.nginx),('busy_ports',None),('choose_site',self.site_dir/n.SITE_NAME),('ensure_certbot',Path('/usr/bin/certbot')),('selinux_prepare',None),('nginx_reload',None),('ipv6',False),('public_get',(200,b'OK'))]:stack.enter_context(patch.object(n,name,return_value=value))
        stack.enter_context(patch('builtins.input',return_value='probe.example.com'))
        stack.enter_context(patch.object(n.subprocess,'run',return_value=subprocess.CompletedProcess([],1)))
        stack.enter_context(patch.object(n,'run'));return stack
    def test_acme_failure_restores_site_leaves_origin(self):
        calls=[]
        with self.setup_context(),patch.object(n,'issue',side_effect=ValueError('ACME rejected')),self.assertRaises(ValueError):n.setup(self.settings,lambda *x:calls.append(x),self.state)
        self.assertEqual(calls,[]);self.assertFalse((self.site_dir/n.SITE_NAME).exists());self.assertFalse((n.CONF/'site.json').exists())
    def test_health_failure_restores_origin(self):
        calls=[]
        with self.setup_context(),patch.object(n,'issue',return_value='http'),patch.object(n,'certificate_valid',return_value=True),patch.object(n,'local_https',side_effect=ValueError('health failed')),patch.object(n.time,'sleep'),self.assertRaises(ValueError):n.setup(self.settings,lambda *x:calls.append(x),self.state)
        self.assertEqual(calls,[('https://probe.example.com',19281),(self.settings['origin'],19281)]);self.assertFalse((self.site_dir/n.SITE_NAME).exists())
    def test_timer_failure_rolls_back(self):
        calls=[]
        with self.setup_context(),patch.object(n,'issue',return_value='http'),patch.object(n,'certificate_valid',return_value=True),patch.object(n,'local_https'),patch.object(n,'install_renewal',side_effect=ValueError('timer failed')),self.assertRaises(ValueError):n.setup(self.settings,lambda *x:calls.append(x),self.state)
        self.assertEqual(calls[-1],(self.settings['origin'],19281));self.assertFalse((n.CONF/'site.json').exists());self.assertFalse((self.site_dir/n.SITE_NAME).exists())
    def test_success_closes_direct_listener_saves_ownership(self):
        calls=[]
        with self.setup_context(),patch.object(n,'issue',return_value='http'),patch.object(n,'certificate_valid',return_value=True),patch.object(n,'local_https'),patch.object(n,'install_renewal'):n.setup(self.settings,lambda *x:calls.append(x),self.state)
        self.assertEqual(calls,[('https://probe.example.com',19281)])
        config=json.loads((n.CONF/'site.json').read_text());self.assertEqual(config['site_sha256'],hashlib.sha256((self.site_dir/n.SITE_NAME).read_bytes()).hexdigest());self.assertEqual(config['method'],'http')
    def test_rerun_is_idempotent(self):
        (self.site_dir/n.SITE_NAME).write_text('old');n.write(n.CONF/'site.json',json.dumps({'domain':'probe.example.com','method':'cloudflare'}))
        settings={'origin':'https://probe.example.com','port':19281,'listen':'127.0.0.1:19281'};calls=[]
        with self.setup_context(),patch.object(n,'issue') as issue,patch.object(n,'certificate_valid',return_value=True),patch.object(n,'local_https'),patch.object(n,'install_renewal'):n.setup(settings,lambda *x:calls.append(x),self.state)
        issue.assert_not_called();self.assertEqual(calls,[])
    def test_repository_does_not_overwrite(self):
        path=self.root/'repo';path.write_text('foreign')
        with self.assertRaises(ValueError):n.reserved_write(path,'managed')
        self.assertEqual(path.read_text(),'foreign')

    def test_renewal_with_unchanged_certificate_does_not_reload(self):
        domain='probe.example.com';cert,key=n.cert_paths(domain);cert.parent.mkdir(parents=True)
        cert.write_text('same certificate');key.write_text('private fixture')
        binary=n.VENV/'bin'/'certbot';binary.parent.mkdir(parents=True);binary.write_text('fixture')
        n.write(n.CONF/'site.json',json.dumps({'domain':domain,'nginx':self.nginx,'site':str(self.site_dir/n.SITE_NAME)}))
        with patch.object(n,'discover_nginx',return_value=self.nginx),patch.object(n,'choose_site'),patch.object(n,'certbot_run'),patch.object(n,'certificate_valid',return_value=True),patch.object(n,'nginx_reload') as reload:
            n.renew();reload.assert_not_called()
    def test_foreign_renewal_unit_is_not_overwritten(self):
        unit=n.SYSTEMD/n.TIMER;unit.write_text('foreign timer')
        tx=n.Transaction()
        with patch.object(n,'run'),self.assertRaises(ValueError):n.install_renewal(tx)
        tx.rollback();self.assertEqual(unit.read_text(),'foreign timer')
    def test_certbot_process_environment_is_restricted(self):
        with patch.object(n.subprocess,'run',return_value=subprocess.CompletedProcess([],0)) as runner:
            n.certbot_run(['/opt/yuji-probe-certbot/bin/certbot','renew'])
        env=runner.call_args.kwargs['env']
        self.assertEqual(env['CLOUDFLARE_BASE_URL'],'https://api.cloudflare.com/client/v4')
        self.assertNotIn('HTTP_PROXY',env);self.assertNotIn('PYTHONPATH',env)
    def test_existing_foreign_certbot_environment_is_not_used(self):
        n.VENV.mkdir()
        with self.assertRaises(ValueError):n.ensure_certbot()
    def test_uninstall_refuses_modified_vhost_before_deletion(self):
        site=self.site_dir/n.SITE_NAME;site.write_text('user modified')
        n.write(n.CONF/'site.json',json.dumps({'domain':'probe.example.com','site':str(site),'site_sha256':'0'*64}))
        with patch.object(n,'discover_nginx',return_value=self.nginx),patch.object(n,'nginx_check',return_value=self.dump()),patch.object(n,'run') as runner,self.assertRaises(ValueError):
            n.unconfigure()
        runner.assert_not_called();self.assertEqual(site.read_text(),'user modified')


    def make_certificate(self):
        cert,key=n.cert_paths('probe.example.com');cert.parent.mkdir(parents=True)
        subprocess.run(['openssl','req','-x509','-newkey','ec','-pkeyopt','ec_paramgen_curve:prime256v1','-nodes','-days','1','-subj','/CN=probe.example.com','-addext','subjectAltName=DNS:probe.example.com','-keyout',str(key),'-out',str(cert)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        key.chmod(0o600);return cert,key
    def test_certificate_hostname_and_expiry_are_both_checked(self):
        cert,key=self.make_certificate()
        self.assertTrue(n.certificate_valid('probe.example.com'))
        self.assertFalse(n.certificate_valid('probe.example.com',seconds=2*86400))
        with patch.object(n,'cert_paths',return_value=(cert,key)):
            self.assertFalse(n.certificate_valid('wrong.example.com'))
    def test_private_key_permissions_rejected(self):
        cert,key=self.make_certificate();key.chmod(0o644)
        with self.assertRaises(ValueError):n.certificate_valid('probe.example.com')
    def test_nginx_syntax_failure_restores_site(self):
        with self.setup_context(),patch.object(n,'nginx_reload',side_effect=[ValueError('syntax error'),None]),patch.object(n,'issue') as issue,self.assertRaises(ValueError):
            n.setup(self.settings,lambda *_:None,self.state)
        issue.assert_not_called();self.assertFalse((self.site_dir/n.SITE_NAME).exists())


    def test_package_install_failure_removes_only_added_repository_files(self):
        ours=self.root/'new-repo';other=self.root/'existing-repo';other.write_text('original')
        def failing(repository):
            repository(other,'original')
            repository(ours,'new official repository')
            raise ValueError('package manager failed')
        with patch.object(n,'_install_nginx',side_effect=failing),self.assertRaises(ValueError):n.install_nginx()
        self.assertFalse(ours.exists());self.assertEqual(other.read_text(),'original')

if __name__=='__main__':
    if os.geteuid()!=0:raise SystemExit('Use root in a disposable test environment')
    unittest.main(verbosity=2)
