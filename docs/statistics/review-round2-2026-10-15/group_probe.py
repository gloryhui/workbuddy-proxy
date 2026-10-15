import sys,time,json
from pathlib import Path
import helper as r
t=r.t;results=[]
with r.fixture() as (d,p,process,log):
 for model in ['wbp-ok','model-json-error','model-json-invalid']:t.relay(p,model,stream='json' not in model)
 time.sleep(.4);f=r.filter_now()
 first=t.stats(d,p,'stats_breakdown',filter=f,group_by='model',sort='total_tokens',limit=1)
 cursor=first['next_cursor']
 print('first page:',json.dumps({'groups':first['total_groups'],'keys':[i['key'] for i in first['items']],'next_cursor':cursor}),flush=True)
 for name,c in [('server_cursor',cursor),('column_key_to_reach_sql',dict(cursor,key='upstream_model'))]:
  try:
   page=t.stats(d,p,'stats_breakdown',filter=f,group_by='model',sort='total_tokens',limit=1,cursor=c)
   result={'case':name,'accepted':True,'keys':[i['key'] for i in page['items']]}
  except AssertionError as e:result={'case':name,'accepted':False,'error':str(e)}
  results.append(result);print(json.dumps(result,ensure_ascii=False),flush=True)
Path(__file__).with_name('group-results.json').write_text(json.dumps(results,ensure_ascii=False,indent=2)+'\n')
