"""Immutable cache history must recover after an earlier workflow failed."""
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ci_run


class CacheHistoryTests(unittest.TestCase):
    def setUp(self):
        self.env = patch.dict(os.environ, GITHUB_RUN_ID='456', GITHUB_RUN_ATTEMPT='1',
            GITHUB_ACTIONS='true', GH_TOKEN='fixture', GITHUB_REPOSITORY='owner/repo',
            GITHUB_REF='refs/pull/64/merge', CI_FORCE_RECHECK='false')
        self.env.start()
        self.addCleanup(self.env.stop)
        self.task = dict(kind='quality', name='server')
        self.key = 'exact-full-input-hash'

    def response(self, value):
        return io.BytesIO(json.dumps(value).encode())

    def test_failed_origin_cannot_poison_future_success_for_unchanged_inputs(self):
        ref = os.environ['GITHUB_REF']
        old = self.key + '-run-123-1'
        caches = {old: {'key': self.key, 'task': self.task, 'conclusion': 'success', 'run': '123'}}
        def api(request, **_kwargs):
            if '/actions/caches?' in request.full_url:
                return self.response({'actions_caches': [{'key': key, 'ref': ref} for key in reversed(caches)]})
            return self.response({'status': 'completed', 'conclusion':
                'failure' if request.full_url.endswith('/123') else 'success'})
        with tempfile.TemporaryDirectory() as temporary, patch.object(ci_run.urllib.request, 'urlopen', side_effect=api):
            directory = Path(temporary)
            storage, restored = ci_run.cache_storage(self.task, self.key)
            self.assertEqual(restored, old)
            self.assertNotIn(storage, caches)
            record = directory / 'success.json'
            record.write_text(json.dumps(caches[restored]))
            self.assertFalse(ci_run.reusable(directory, self.key, self.task))
            # A real successful rerun records its new origin under a fresh key.
            ci_run.record(directory, self.key, self.task)
            caches[storage] = json.loads(record.read_text())
            self.assertEqual(caches[old]['run'], '123')
            with patch.dict(os.environ, GITHUB_RUN_ID='789'):
                next_storage, restored = ci_run.cache_storage(self.task, self.key)
            self.assertNotIn(next_storage, caches)
            self.assertEqual(restored, storage)
            record.write_text(json.dumps(caches[restored]))
            self.assertTrue(ci_run.reusable(directory, self.key, self.task))
            self.assertFalse(ci_run.cached(directory, 'different-input-hash', self.task))

    def test_history_requires_full_input_key_and_same_pr_ref(self):
        rows = [{'key': self.key + '-other-run-999-1', 'ref': os.environ['GITHUB_REF']},
                {'key': self.key + '-run-999-1', 'ref': 'refs/pull/65/merge'},
                {'key': self.key + '-run-123-1', 'ref': os.environ['GITHUB_REF']}]
        with patch.object(ci_run.urllib.request, 'urlopen', return_value=self.response({'actions_caches': rows})):
            self.assertEqual(ci_run.cache_storage(self.task, self.key)[1], self.key + '-run-123-1')
        with patch.dict(os.environ, GITHUB_RUN_ATTEMPT='2'):
            with patch.object(ci_run.urllib.request, 'urlopen', side_effect=OSError('Unavailable')):
                self.assertEqual(ci_run.cache_storage(self.task, self.key), (self.key + '-run-456-2', ''))

    def test_build_keeps_one_checksum_verified_blob_without_history_requests(self):
        with patch.object(ci_run.urllib.request, 'urlopen') as request:
            self.assertEqual(ci_run.cache_storage(dict(kind='build'), self.key), (self.key, ''))
            request.assert_not_called()
