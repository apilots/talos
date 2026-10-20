"""HTTP regression contracts for the deterministic auxiliary model."""
import json
import re
import threading
import unittest
import urllib.error
import urllib.request
from http.server import ThreadingHTTPServer

import fixture


class AuxiliaryModelTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = ThreadingHTTPServer(('127.0.0.1', 0), fixture.Fixture)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        title_source = (fixture.ROOT / 'talos-rs/crates/orchestration/service/src/session/title.rs').read_text()
        memory_source = (fixture.ROOT / 'talos-rs/crates/orchestration/service/src/user_workspace_memory/reflection.rs').read_text()
        # Use the actual product prompts so fixture contract drift fails this transport test.
        cls.title_prompt = re.search(r'const TITLE_PROMPT: &str = "(.*?)";', title_source).group(1)
        cls.memory_prompt = re.search(r'const REFLECTION_PROMPT: &str = r#"(.*?)"#;', memory_source, re.S).group(1)

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=5)

    def request(self, prompt, payload, **overrides):
        body = {
            'model': 'browser-fixture',
            'messages': [
                {'role': 'system', 'content': prompt},
                {'role': 'user', 'content': json.dumps(payload)},
            ],
            **overrides,
        }
        request = urllib.request.Request(
            f'http://127.0.0.1:{self.server.server_port}/v1/chat/completions',
            data=json.dumps(body).encode(), headers={'Content-Type': 'application/json'})
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        try:
            response = opener.open(request, timeout=5)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.status, json.load(response)

    def test_title_returns_task_name_without_tools(self):
        payload = {'user_message': 'BROWSER_WORKER_ACCEPTANCE', 'assistant_response': 'Done'}
        for tools in ({}, {'tools': []}):
            with self.subTest(tools=tools):
                status, body = self.request(self.title_prompt, payload, **tools)
                self.assertEqual(status, 200)
                self.assertEqual(body['choices'][0]['message']['content'], fixture.SESSION_TITLE)

    def test_memory_keeps_its_json_contract(self):
        status, body = self.request(self.memory_prompt, {
            'currentMemory': '', 'userMessage': 'Routine browser task', 'assistantResponse': 'Done'})
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(body['choices'][0]['message']['content']), {'operation': 'noop'})

    def test_user_text_cannot_choose_auxiliary_workload(self):
        status, body = self.request(self.memory_prompt, {
            'currentMemory': '', 'userMessage': self.title_prompt, 'assistantResponse': 'Done'})
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(body['choices'][0]['message']['content']), {'operation': 'noop'})

    def test_unknown_auxiliary_request_fails_explicitly(self):
        status, body = self.request('Summarize this conversation.', {'user_message': 'Hello'})
        self.assertEqual(status, 400)
        self.assertEqual(body['error']['type'], 'invalid_request_error')

    def test_title_cannot_receive_memory_payload(self):
        status, _ = self.request(self.title_prompt, {
            'currentMemory': '', 'userMessage': 'Hello', 'assistantResponse': 'Done'})
        self.assertEqual(status, 400)

    def test_memory_rejects_unserialized_rust_field_names(self):
        status, _ = self.request(self.memory_prompt, {
            'current_memory': '', 'user_message': 'Hello', 'assistant_response': 'Done'})
        self.assertEqual(status, 400)

    def test_malformed_messages_fail_instead_of_returning_noop(self):
        for messages in ([], [None, None], [{'role': 'user', 'content': self.title_prompt}]):
            with self.subTest(messages=messages):
                status, _ = self.request(self.title_prompt, {}, messages=messages)
                self.assertEqual(status, 400)

    def test_docker_background_home_prefix_keeps_file_steps_and_native_flow(self):
        names = ['browser_tab', 'browser_navigate', 'browser_state', 'browser_fill',
                 'browser_click', 'browser_screenshot', 'browser_upload', 'browser_download']
        catalog = [{'type': 'function', 'function': {'name': 'mcp__talos_browser__' + name}}
                   for name in names]
        elements = [{'element': 'name', 'tag': 'input', 'attributes': {}},
                    {'element': 'submit', 'tag': 'button', 'attributes': {}},
                    {'element': 'upload', 'tag': 'input', 'attributes': {'type': 'file'}},
                    {'element': 'download', 'tag': 'a', 'attributes': {}}]
        previous = list(fixture.calls)
        self.addCleanup(lambda: fixture.calls.__setitem__(slice(None), previous))
        for keep_home in (False, True):
            with self.subTest(keep_home=keep_home):
                fixture.calls.clear()
                messages = [{'role': 'user', 'content': ('BROWSER_KEEP_HOME ' if keep_home else '')
                             + 'BROWSER_UPLOAD_FILE_ID=attached-file'}]
                observed = []
                for _ in range(14 if keep_home else 13):
                    status, body = self.request('', {}, messages=messages, tools=catalog)
                    self.assertEqual(status, 200)
                    message = body['choices'][0]['message']
                    call = message['tool_calls'][0]
                    observed.append((call['function']['name'].split('__')[-1],
                                     json.loads(call['function']['arguments'])))
                    messages.extend([message, {'role': 'tool', 'tool_call_id': call['id'],
                                               'content': json.dumps({'elements': elements})}])
                if keep_home:
                    self.assertEqual(observed[0], ('browser_tab', {'action': 'open', 'url': 'about:blank'}))
                else:
                    self.assertEqual(observed[0][0], 'browser_navigate')
                uploads = [args['fileId'] for name, args in observed if name == 'browser_upload']
                self.assertEqual(uploads, ['not-attached-to-this-turn', 'attached-file'])
                clicks = [args['element'] for name, args in observed if name == 'browser_click']
                self.assertEqual(clicks, ['submit', 'download'])
                self.assertEqual(observed[-1][0], 'browser_download')


if __name__ == '__main__':
    unittest.main()
