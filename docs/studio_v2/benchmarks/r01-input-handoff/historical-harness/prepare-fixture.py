from pathlib import Path
import json,os,plistlib,subprocess,hashlib
r=Path(__file__).resolve().parent;b=Path.cwd()/'docs/studio_v2/benchmarks'
source=b/'b05-restart-identity-guard.m.txt';(r/'restart-identity-guard.m').write_bytes(source.read_bytes());cmd=['xcrun','clang','-dynamiclib','-fobjc-arc','-framework','AppKit',str(r/'restart-identity-guard.m'),'-o',str(r/'restart-identity-guard.dylib')];z=subprocess.run(cmd,capture_output=True,text=True);assert z.returncode==0,z.stderr;(r/'guard-build.json').write_text(json.dumps({'exit_code':z.returncode,'output':z.stdout+z.stderr,'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'scope':'Exact B05 audited QA-only identity guard, configured deny-only. Never routes to real WorkBuddy.'},indent=2)+'\n')
target=r/'Native Restart Denied.app';(target/'Contents').mkdir(parents=True);target_id='com.workbuddy.codexreview.b05target.r01ci7zkz3s';(target/'Contents/Info.plist').write_bytes(plistlib.dumps({'CFBundleIdentifier':target_id,'CFBundleExecutable':'never-launched','CFBundlePackageType':'APPL'}));(r/'restart-routing.json').write_text('{"route_owned_target":false}\n');env={'DYLD_INSERT_LIBRARIES':str(r/'restart-identity-guard.dylib'),'B05_OWN_ROOT':str(r),'B05_TARGET_APP':str(target),'B05_TARGET_ID':target_id,'B05_GUARD_MODE':str(r/'restart-routing.json'),'B05_GUARD_AUDIT':str(r/'restart-guard-audit.jsonl'),'B05_GUARD_NONCE':'r01-deny-only-ci7zkz3s'};(r/'restart-env-private.json').write_text(json.dumps(env)+'\n');os.chmod(r/'restart-env-private.json',0o600)
s=(b/'b04-fixture-background.py.txt').read_text().replace('synthetic-b04','synthetic-r01').replace('WorkBuddy Proxy B04 Review','WorkBuddy Proxy R01 Review')
marker="bundle=folder/'WorkBuddy Proxy R01 Review.app'";assert marker in s;s=s.replace(marker,"inject=json.loads((pathlib.Path(__file__).resolve().parent/'restart-env-private.json').read_text());launch_environment.update(inject)\n"+marker)
needle="        update_mode=mode.get('update','real');status=200;controlled=False";assert needle in s
hold='''        # Audited test boundary gate: records arrivals before response. Only
        # own synthetic management requests are held; Core binary is unchanged.
        gatepath=folder/'gate.json';gate=json.loads(gatepath.read_text()) if gatepath.exists() else {}
        nonce=gate.get('nonce','');held=gate.get('command')==name and bool(nonce)
        with bridge_audit_lock:
            with (folder/'management-arrivals.jsonl').open('a') as audit:audit.write(json.dumps({'at_ms':int(time.time()*1000),'command':name,'held':held,'nonce':nonce if held else None,'provider_id':payload.get('provider_id')})+'\\n')
        if held:
            assert nonce.replace('-','').replace('_','').isalnum() and len(nonce)<64
            awaiting=folder/('gate-awaiting-'+nonce+'.json');awaiting.write_text(json.dumps({'command':name,'nonce':nonce,'pid':os.getpid(),'at_ms':int(time.time()*1000)})+'\\n')
            deadline=time.monotonic()+60
            while not (folder/('gate-release-'+nonce+'.json')).exists():
                if time.monotonic()>deadline:
                    data=b'{"message":"Synthetic busy gate timed out"}';self.send_response(503);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data);return
                time.sleep(.025)
'''
s=s.replace(needle,hold+needle)
# Bound the public updater simulation to quick deterministic completion after
# explicit gate release; holding/duplicates remain actual native UI behavior.
s=s.replace("update_mode=mode.get('update','real')","update_mode=mode.get('update','latest')")
assert 'bridge_server.shutdown()' in s
s=s.replace("manifest.update(copy_inheritance_fixture=options.copy_inheritance", "manifest['restart_guard']={'nonce':inject['B05_GUARD_NONCE'],'audit':inject['B05_GUARD_AUDIT'],'deny_real_WorkBuddy':True}\nmanifest.update(copy_inheritance_fixture=options.copy_inheritance")
(r/'fixture-background.py').write_text(s);compile(s,str(r/'fixture-background.py'),'exec');(r/'preflight-guard.py').write_bytes((b/'b05-preflight-guard.py.txt').read_bytes())
# Reuse the driver lifecycle and protected history baseline without B04 modes.
s=(b/'b04-native-driver.py.txt').read_text().replace("phase=sys.argv[1];assert phase in ['look','main']","phase='main'").replace("'b04-r-9n9fkx-'","'r01-ci7zkz3s-'").replace("r/('baseline-ui' if phase=='look' else 'candidate-ui')","r/'baseline-ui'").replace("{'update':'latest','delay':2,'language_failure':phase=='look'}","{'update':'latest','delay':0,'language_failure':False}").replace('B04 '+"' +phase",'R01 '+"' +phase")
s=s.replace("'controlled_update_scope':'Loopback management bridge only. Core/routing/config unchanged; simulated updater results are not public GitHub/Core-updater E2E.'","'controlled_update_scope':'Loopback UI-boundary gate/update only. Real Core forwards all ordinary operations. Not public GitHub updater E2E.'")
(r/'native-driver.py').write_text(s);compile(s,str(r/'native-driver.py'),'exec');print('R01 protected Core + deny-native-restart + exact-arrival controlled-busy fixture prepared, not yet launched.')
