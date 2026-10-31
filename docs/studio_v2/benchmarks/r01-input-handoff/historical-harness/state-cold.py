from pathlib import Path
import argparse,copy,datetime,hashlib,json,os,sqlite3,time,urllib.request
r=Path(__file__).resolve().parent;f=r/'r01-ci7zkz3s-cold';p=argparse.ArgumentParser();p.add_argument('action',choices=['observe','arm','release','count','disarm']);p.add_argument('stage');p.add_argument('--command',choices=['test','check_update']);p.add_argument('--nonce');p.add_argument('--canonical-save',action='store_true');p.add_argument('--unicode-model-name',action='store_true');p.add_argument('--expected-language',choices=['auto','zh','en']);p.add_argument('--expected-skin',choices=['monochrome','blue','violet','rose','amber','teal','studio_indigo']);p.add_argument('--arrivals',type=int,default=1);a=p.parse_args();assert a.stage.replace('_','').replace('-','').isalnum()
if a.action in ['arm','release','count']:
 assert a.nonce and a.nonce.replace('_','').replace('-','').isalnum() and len(a.nonce)<64
 if a.action=='arm':
  assert a.command;assert not (f/('gate-awaiting-'+a.nonce+'.json')).exists();(f/'gate.json').write_text(json.dumps({'command':a.command,'nonce':a.nonce})+'\n');print('armed '+a.command+' '+a.nonce);raise SystemExit
 if a.action=='release':
  assert (f/('gate-awaiting-'+a.nonce+'.json')).exists();(f/('gate-release-'+a.nonce+'.json')).write_text('{}\n');print('released '+a.nonce);raise SystemExit
 rows=[json.loads(x) for x in (f/'management-arrivals.jsonl').read_text().splitlines()];selected=[x for x in rows if x.get('nonce')==a.nonce];assert len(selected)==a.arrivals,(a.nonce,len(selected));assert (f/('gate-awaiting-'+a.nonce+'.json')).exists();(f/('arrival-proof-'+a.stage+'.json')).write_text(json.dumps({'nonce':a.nonce,'count':len(selected),'expected':a.arrivals,'arrivals':selected,'release_exists':(f/('gate-release-'+a.nonce+'.json')).exists(),'scope':'Actual arrivals at own management boundary, not inferred from screenshot. Unreleased gate proves original job still waiting; after release compare again after UIcompletion to catch queued duplicates.'},indent=2)+'\n');print('arrival_count '+str(len(selected)));raise SystemExit
if a.action=='disarm':(f/'gate.json').write_text('{"command":null,"nonce":""}\n');print('disarmed');raise SystemExit
cfgpath=f/'config/config.json';raw=cfgpath.read_bytes();cfg=json.loads(raw);inst=json.loads((f/'config/instance.json').read_text());ready=json.loads((f/'cua-launch-ready.json').read_text());assert inst['pid']==ready['core_pid'];opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
def call(name):
 req=urllib.request.Request('http://127.0.0.1:%d/_control/action'%inst['port'],data=json.dumps({'command':name}).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+inst['management_key']})
 with opener.open(req,timeout=12) as z:return json.loads(json.loads(z.read())['message'])
def norm(v):
 v=copy.deepcopy(v)
 for provider in v['providers']:
  provider.setdefault('protocol','openai_chat');provider.setdefault('protocol_settings',{'authentication':'protocol_default','anthropic_version':'2023-06-01','default_max_output_tokens':4096,'reasoning_profile':{'mode':'none','budgets':{},'levels':{}},'responses_efforts':{}})
  for m in provider['models']:m.setdefault('capabilities',None)
 return v
sha=lambda x:hashlib.sha256(x).hexdigest();modelraw=(f/'home/.workbuddy/models.json').read_bytes();snap=call('snapshot');settings=call('stats_settings_get');settings={k:settings[k] for k in ['enabled','retention_days','default_currency','revision']};prices={v['rule_id']:{k:w for k,w in v.items() if k!='referenced'} for v in call('stats_prices_list')['items']};assert call('stats_health')['state']=='ready'
with sqlite3.connect('file:'+str(f/'config/usage.sqlite3')+'?mode=ro',uri=True) as db:
 db.row_factory=sqlite3.Row;db.execute('BEGIN');history={t:{x['request_id']:dict(x) for x in db.execute('SELECT * FROM '+t)} for t in ['usage_requests','usage_costs']}
basefile=f/'r01-baseline-private.json';checkpoint=f/'r01-checkpoint-private.json'
if a.stage=='initial':
 assert not basefile.exists();base={'cfg':cfg,'models_sha256':sha(modelraw),'settings':settings,'prices':prices,'history':history};basefile.write_text(json.dumps(base)+'\n');os.chmod(basefile,0o600);previous={'cfg_sha256':sha(raw)}
else:base=json.loads(basefile.read_text());previous=json.loads(checkpoint.read_text())
# Accept only the separately proven exact existing Core midnight catalogue transition.
price_transition_file=r/'daily-price-transition.json';price_transition=None
price_preserved=prices==base['prices']
if not price_preserved and price_transition_file.exists():
 price_transition=json.loads(price_transition_file.read_text());digest=lambda v:sha(json.dumps(v,sort_keys=True,separators=(',',':')).encode())
 price_preserved=digest(base['prices'])==price_transition['baseline_digest'] and digest(prices)==price_transition['accepted_digest']
expected_cfg=copy.deepcopy(base['cfg'])
expected_cfg['port']=json.loads((f/'cold-copy-proof.json').read_text())['cold_port']
if a.expected_language is not None:expected_cfg['language']=a.expected_language
if a.unicode_model_name:
 targets=[m for provider in expected_cfg['providers'] if provider['id']==base['cfg']['providers'][0]['id'] for m in provider['models'] if m['model']=='gpt-6']
 assert len(targets)==1;targets[0]['name']='R01你好🧪 e\u0301'
if a.expected_skin is not None:assert json.loads((f/'config/appearance.json').read_text())=={'version':1,'skin':a.expected_skin},'Unexpected appearance state'
assert norm(cfg)==norm(expected_cfg) and sha(modelraw)==base['models_sha256'] and settings==base['settings'] and price_preserved,'Unexpected business/config/models/price/settings change'
if not a.canonical_save:assert sha(raw)==previous['cfg_sha256'],'Unexpected config file write'
for label,protected in [('original',base['history']),('previous_completed',previous.get('history',base['history']))]:
 for t,rows in protected.items():assert all(history[t].get(k)==v for k,v in rows.items()),label+' full-field history changed'
ui_pid=json.loads((f/'config/gpui.json').read_text())['pid'];guard=[json.loads(x) for x in (r/'restart-guard-audit.jsonl').read_text().splitlines()];events=[x for x in guard if x['process_pid']==ui_pid];assert events and events[0]['event']=='guard_installed' and events[0]['nonce']=='r01-deny-only-ci7zkz3s';assert [x['event'] for x in events]==['guard_installed'],'Unexpected native restart attempt'
for secret in [cfg['local_key'],inst['management_key'],*[v['api_key'] for v in cfg['providers']]]:assert secret not in json.dumps(snap['logs'])
out={'at':datetime.datetime.now().astimezone().isoformat(),'stage':a.stage,'passed':True,'core_pid':inst['pid'],'ui_pid':ui_pid,'cfg_sha256':sha(raw),'cfg_semantics_models_prices_settings_preserved':True,'exact_existing_core_daily_price_transition':price_transition['catalog'] if price_transition else None,'history':{t:{'rows':len(rows),'original_all_fields_preserved':True} for t,rows in history.items()},'counts':{k:snap[k] for k in ['providers','models','requests','url']},'guard_events':[x['event'] for x in events],'canonical_save_declared':a.canonical_save,'expected_unicode_model_name':a.unicode_model_name,'expected_language':a.expected_language,'expected_skin':a.expected_skin,'scope':'Read-only original Core via loopback management bridge. Baseline business semantics/models/prices/settings preserved; only declared format/default serialization from existing Save/Test path allowed. No native GUI action or screenreader result inferred.'};(f/'stage-proofs').mkdir(exist_ok=True);(f/'stage-proofs'/(a.stage+'.json')).write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n');checkpoint.write_text(json.dumps({'cfg_sha256':sha(raw),'history':history})+'\n');os.chmod(checkpoint,0o600);print(json.dumps({'stage':a.stage,'passed':True,'ui_pid':ui_pid,'history_rows':len(history['usage_requests'])}))
