import helper as r
import json
from pathlib import Path
t=r.t;out=[]
with r.fixture() as (d,p,process,log):
 fields=dict(provider_id='provider-a',upstream_model='model-ok',currency='USD',input_mode='flat',input_rate_nanos='2000000000',output_rate_nanos='8000000000',label='edge')
 first=t.stats(d,p,'stats_price_save',**fields)
 fields['input_rate_nanos']='99000000000'
 try:
  changed=t.stats(d,p,'stats_price_save',**fields,expected_revision=first['revision'])
  out.append({'case':'new_with_existing_revision_but_no_id','accepted':True,'before':first,'after':changed})
 except AssertionError as e:out.append({'case':'new_with_existing_revision_but_no_id','accepted':False,'error':str(e)})
 current=t.stats(d,p,'stats_prices_list')['items'][0]
 try:
  response=t.stats(d,p,'stats_price_delete',price_id=current['rule_id'],expected_revision=current['revision'])
  out.append({'case':'canonical_delete','accepted':True,'response':response})
 except AssertionError as e:out.append({'case':'canonical_delete','accepted':False,'error':str(e)})
 print(json.dumps(out,ensure_ascii=False),flush=True)
Path(__file__).with_name('price-edge-results.json').write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n')
