#!/usr/bin/env python3
"""Loopback-only OpenAI-compatible models mock for SC03; fake credential."""
import json, socket
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
CAP = Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t09-x05-repeat-corrected-001')
LOG = CAP / 'logs/mock-provider.log'
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        with open(LOG, 'a') as f:
            f.write('GET %s auth=%s\n' % (self.path, 'present' if self.headers.get('Authorization') else 'none'))
        if self.path.rstrip('/') in ('/v1/models', '/models'):
            body = json.dumps({'object':'list','data':[
                {'id':'mock-model-a','object':'model','created':0,'owned_by':'local'},
                {'id':'mock-model-b','object':'model','created':0,'owned_by':'local'}]}).encode()
            self.send_response(200); self.send_header('Content-Type','application/json')
            self.send_header('Content-Length',str(len(body))); self.end_headers(); self.wfile.write(body)
        else:
            self.send_response(404); self.end_headers()
    def log_message(self, *a): pass
s=socket.socket(); s.bind(('127.0.0.1',0)); port=s.getsockname()[1]; s.close()
(CAP/'run/mock-provider.port').write_text(str(port))
HTTPServer(('127.0.0.1',port), H).serve_forever()
