#!/usr/bin/env python3
"""Offline price-catalogue mock for T08-RESIDUAL-003.

Serves the models.dev-shaped JSON at /api.json for the isolated core. The mode
file decides the reply, so the GUI can show all three R02 states without any
network access:

  ok    200 + catalogue immediately
  slow  200 + catalogue after 3 seconds (the button stays "updating")
  fail  500 (the core reports a failed update and the UI must recover)

The URL is only accepted by the core because it is http://127.0.0.1 (see
src/price_catalog.rs); no upstream service is contacted.
"""
import json
import sys
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

RUN = Path(sys.argv[1])
MODE = RUN / 'mock-mode'
CATALOG = {
    'opencode': {
        'api': 'https://opencode.test/v1',
        'models': {
            'deepseek-v4.1-flash': {
                'canonical_model_id': 'opencode/deepseek-v4.1-flash',
                'cost': {'input': 0.15, 'output': 0.6,
                         'cache_read': 0.003, 'cache_write': 0},
            },
            'unmatched-catalogue-model': {
                'cost': {'input': 1, 'output': 2},
            },
        },
    },
}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        if self.path.split('?')[0] != '/api.json':
            self.send_response(404)
            self.end_headers()
            return
        try:
            mode = MODE.read_text().strip() or 'ok'
        except FileNotFoundError:
            mode = 'ok'
        if mode == 'slow':
            time.sleep(3.0)
        if mode == 'fail':
            self.send_response(500)
            self.send_header('Content-Type', 'text/plain')
            self.end_headers()
            self.wfile.write(b'mock catalogue failure')
            return
        body = json.dumps(CATALOG).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    (RUN / 'mock-ready').write_text('ready')
    HTTPServer(('127.0.0.1', int(sys.argv[2])), Handler).serve_forever()


if __name__ == '__main__':
    main()
