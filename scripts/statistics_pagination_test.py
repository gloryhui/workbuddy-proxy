#!/usr/bin/env python3
"""Isolated real-core coverage of all list cursors and currency snapshots.

Synthetic history is inserted only in a temporary database. No real user profile,
credentials or provider is used. This complements native GPUI action verification.
"""
from contextlib import closing
import argparse
import json
from pathlib import Path
import sqlite3
import tempfile
import threading
import time
import statistics_test as t


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--exe',default='target/release/workbuddy-proxy')
    args=parser.parse_args()
    server=t.ThreadingHTTPServer(('127.0.0.1',0),t.Mock)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='wbp-list-contracts-') as tmp:
            folder=Path(tmp);port=t.free_port()
            t.write_config(folder,port,'http://127.0.0.1:%s/v1'%server.server_port)
            process,log,_=t.start_core(args.exe,folder,port)
            try:
                t.wait_for(lambda:t.stats(folder,port,'stats_health')['state']=='ready','ready')
                t.relay(port,'wbp-ok');time.sleep(.4)
                with closing(sqlite3.connect(folder/'usage.sqlite3')) as db, db:
                    db.execute('PRAGMA foreign_keys=ON');db.row_factory=sqlite3.Row
                    req=dict(db.execute('SELECT * FROM usage_requests LIMIT 1').fetchone());req.pop('id')
                    cost=dict(db.execute('SELECT * FROM usage_costs LIMIT 1').fetchone())
                    def put(table,row):
                        keys=list(row)
                        db.execute('INSERT INTO '+table+' ('+','.join(keys)+') VALUES ('+','.join('?' for _ in keys)+')',[row[k] for k in keys])
                    for i in range(321):
                        req['request_id']=cost['request_id']='list-%04d'%i
                        req['provider_id']='history-provider-%03d'%i;req['provider_name']='History %03d'%i
                        req['upstream_model']='history-model-%03d'%i;put('usage_requests',req);put('usage_costs',cost)
                        db.execute("INSERT INTO stats_model_prices(price_id,provider_id,upstream_model,currency,input_mode,input_rate_nanos,output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,label,revision,created_at_ms,updated_at_ms) VALUES(?,?,?,'CNY','flat',2000000000,8000000000,0,0,'fixture',1,?,?)",('price-%04d'%i,'provider-a','history-model-%03d'%i,t.now_ms(),t.now_ms()))
                filt={'from_ms':t.now_ms()-3600000,'to_ms':t.now_ms()+60000,'timezone':'UTC'}
                def stats(command,**fields):return t.stats(folder,port,command,**fields)
                for size in [20,50,100]:
                    for group in [None,'provider','model']:
                        cursors=[None];ids=[]
                        for page in range(3):
                            fields=dict(filter=filt)
                            if cursors[page]:fields['cursor']=cursors[page]
                            if group:
                                value=stats('stats_breakdown',group_by=group,sort='total_tokens',limit=size,**fields)
                                page_ids=[r['key'] for r in value['items']]
                            else:
                                value=stats('stats_requests',page_size=size,**fields)
                                page_ids=[r['request_id'] for r in value['items']]
                            assert len(page_ids)==size,(size,group,value)
                            ids.append(page_ids);cursors.append(value['next_cursor'])
                        assert len(set(sum(ids,[])))==size*3
                        for page in [1,0]:
                            fields=dict(filter=filt)
                            if cursors[page]:fields['cursor']=cursors[page]
                            value=stats('stats_breakdown',group_by=group,sort='total_tokens',limit=size,**fields) if group else stats('stats_requests',page_size=size,**fields)
                            assert [r['key' if group else 'request_id'] for r in value['items']]==ids[page]
                        print('PASS real %s pages 1->2->3->2->1 size %s'%(group or 'requests',size),flush=True)
                    # The pricing view keeps exactly one page, using the core's
                    # next cursor. Cover backtracking as well as the partial end
                    # page; a full page alone must not imply another page exists.
                    price_cursors=[None];price_ids=[];after=None
                    while True:
                        fields={'limit':size}
                        if after is not None:fields['after_rule_id']=after
                        value=stats('stats_prices_list',**fields)
                        current=[row['price_id'] for row in value['items']]
                        assert 0<len(current)<=size,(size,value)
                        price_ids.append(current)
                        after=value['next_rule_id']
                        if after is None:break
                        price_cursors.append(after)
                    assert len(set(sum(price_ids,[])))==321
                    assert len(price_ids[-1])==321%size
                    for page in [1,0,1,2,1,0]:
                        fields={'limit':size,'after_rule_id':price_cursors[page]}
                        value=stats('stats_prices_list',**fields)
                        assert [row['price_id'] for row in value['items']]==price_ids[page]
                    exact=stats('stats_prices_list',limit=size,provider_id='provider-a',upstream_model='history-model-320')
                    assert [row['price_id'] for row in exact['items']]==['price-0320']
                    assert exact['next_rule_id'] is None
                    print('PASS real prices fixed pages 1->2->3->2->1, final page and filter reset size %s'%size,flush=True)
                for command,kind,key in [('stats_filters','provider','key'),('stats_filters','model','key'),('stats_prices_list',None,'price_id')]:
                    after=None;seen=[]
                    while True:
                        fields={'limit':100}
                        if kind:
                            fields.update(kind=kind,query='history-')
                            if after:fields['after_key']=after
                        elif after:fields['after_rule_id']=after
                        value=stats(command,**fields);seen.extend(r[key] for r in value['items'])
                        after=value.get('next_key' if kind else 'next_rule_id')
                        if after is None:break
                    assert len(seen)==321 and len(set(seen))==321,(command,kind,len(seen))
                    print('PASS %s %s all 321 options reachable'%(command,kind or ''),flush=True)
                model=stats('stats_filters',kind='model',query='history-model-320',provider_id='history-provider-320',limit=100)
                assert [m['key'] for m in model['items']]==['history-model-320']
                settings=stats('stats_settings_get')
                stats('stats_settings_update',enabled=True,retention_days=180,default_currency='USD',expected_revision=settings['revision'])
                t.relay(port,'wbp-ok');time.sleep(.4)
                with closing(sqlite3.connect(folder/'usage.sqlite3')) as db, db:
                    assert db.execute("SELECT DISTINCT currency FROM usage_costs WHERE request_id LIKE 'list-%'").fetchall()==[('CNY',)]
                    assert db.execute("SELECT currency,total_cost_nanos FROM usage_costs ORDER BY rowid DESC LIMIT 1").fetchone()==('USD',0)
                print('PASS default currency affects only newly admitted zero-price snapshots')
            finally:t.stop_core(process,port,folder,log)
    finally:server.shutdown();server.server_close()
    print('PASS statistics list and settings contracts')


if __name__=='__main__':main()
