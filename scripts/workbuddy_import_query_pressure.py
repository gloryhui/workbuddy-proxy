"""Bounded, continuous real dashboard clients for the scheduling probe.

This is a client load generator, not an alternate importer or report engine.
"""
from collections import deque
import json
import threading
import time
import urllib.error
import urllib.request


class QueryPressure:
    def __init__(self, fixture):
        self.fixture = fixture
        self.stop_event = threading.Event()
        self.lock = threading.Lock()
        self.samples = deque(maxlen=8192)
        self.total = 0
        self.failures = 0
        self.errors = []
        self.minimum_count = None
        self.maximum_count = 0
        self.successes = [0, 0]
        self.started = None
        self.threads = []

    def _client(self, number):
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        at = int(time.time() * 1000)
        fields = {'from_ms': at - 86_400_000, 'to_ms': at + 86_400_000,
                  'timezone': 'UTC', 'include_tests': False,
                  'source_scope': 'builtin', 'capture_method': 'all'}
        payload = json.dumps({'command': 'stats_dashboard', 'filter': fields,
                              'granularity': 'auto'}).encode()
        while not self.stop_event.is_set():
            started = time.monotonic_ns()
            row = {'client': number}
            try:
                request = urllib.request.Request(
                    'http://127.0.0.1:%d/_control/action' % self.fixture.port,
                    data=payload, headers={'Authorization': 'Bearer ' + self.fixture.key,
                                           'Content-Type': 'application/json'})
                try:
                    response = opener.open(request, timeout=10)
                except urllib.error.HTTPError as failure:
                    response = failure
                with response:
                    raw = json.load(response)
                    if not 200 <= response.code < 300:
                        code = str(raw.get('message', '')).split(':', 1)[0]
                        row['error'] = code if code in {
                            'stats_query_timeout', 'stats_query_too_large', 'stats_busy',
                            'stats_unavailable', 'stats_invalid_argument',
                        } else 'unclassified_error'
                    else:
                        row['count'] = json.loads(raw['message'])['summary']['request_count']
                        if row['count'] <= 0:
                            row['error'] = 'unexpected_empty_dashboard'
            except Exception as failure:
                row['error'] = type(failure).__name__
            row['ms'] = (time.monotonic_ns() - started) / 1e6
            with self.lock:
                self.total += 1
                self.samples.append(row)
                if 'error' in row:
                    self.failures += 1
                    if len(self.errors) < 32:
                        self.errors.append(row['error'])
                if 'count' in row:
                    if 'error' not in row:
                        self.successes[number] += 1
                    self.minimum_count = (row['count'] if self.minimum_count is None
                                          else min(self.minimum_count, row['count']))
                    self.maximum_count = max(self.maximum_count, row['count'])
            if 'error' in row:
                return

    def start(self):
        self.started = time.monotonic_ns()
        for number in range(2):
            thread = threading.Thread(target=self._client, args=(number,),
                                      name='dashboard-pressure-%d' % number)
            thread.start()
            self.threads.append(thread)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            with self.lock:
                ready = self.total >= 2 or self.failures > 0
            if ready:
                return
            time.sleep(.02)
        raise AssertionError('continuous dashboard clients did not start')

    def snapshot(self):
        with self.lock:
            return {'completed_requests': self.total, 'failed_requests': self.failures,
                    'live_clients': sum(thread.is_alive() for thread in self.threads)}

    def stop(self, report):
        self.stop_event.set()
        for thread in self.threads:
            thread.join(timeout=15)
        alive = [thread.name for thread in self.threads if thread.is_alive()]
        with self.lock:
            facts = {'scope': 'continuous real dashboard traffic during20 exact-identity '
                             'increment windows; not the formal1M query percentile matrix',
                     'client_count': 2, 'total_requests': self.total,
                     'successful_requests_by_client': list(self.successes),
                     'failed_requests': self.failures, 'error_codes': list(self.errors),
                     'remaining_threads': alive, 'samples': list(self.samples),
                     'discarded_samples': max(0, self.total - len(self.samples)),
                     'minimum_visible_count': self.minimum_count,
                     'maximum_visible_count': self.maximum_count,
                     'wall_ms': (time.monotonic_ns() - self.started) / 1e6}
        report.check('report pressure: actual nonempty dashboard requests succeed',
                     self.total >= 20 and self.failures == 0 and all(self.successes),
                     'requests=%d failures=%d' % (self.total, self.failures))
        report.check('report pressure: dashboard continues reflecting newly committed facts',
                     self.minimum_count is not None and self.maximum_count > self.minimum_count)
        report.check('report pressure: both client threads stop before fixture cleanup', not alive)
        return facts
