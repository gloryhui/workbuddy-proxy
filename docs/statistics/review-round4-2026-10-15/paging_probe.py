import helper as r
import time,json
from pathlib import Path
t=r.t
with r.fixture() as (d,p,process,log):
 for model in ['model-page-a','model-page-b','model-page-c']:
  t.stats(d,p,'stats_price_save',provider_id='provider-a',upstream_model=model,currency='USD',input_mode='flat',input_rate_nanos='2000000000',output_rate_nanos='8000000000',label='paging')
  t.relay(p,model)
 time.sleep(.3);f=r.filter_now()
 first=t.stats(d,p,'stats_breakdown',filter=f,group_by='model',sort='request_count',limit=1)
 for _ in range(2):t.relay(p,'model-page-b')
 time.sleep(.25)
 second=t.stats(d,p,'stats_breakdown',filter=f,group_by='model',sort='request_count',limit=1,cursor=first['next_cursor'])
 out={'case':'group_costs_ignore_as_of','first_as_of':first['as_of_id'],'second_as_of':second['as_of_id'],'key':second['items'][0]['key'],'summary':second['items'][0]['summary'],'expected_cost_nanos':'2800000'}
 print(json.dumps(out,ensure_ascii=False),flush=True)
 Path(__file__).with_name('paging-results.json').write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n')
