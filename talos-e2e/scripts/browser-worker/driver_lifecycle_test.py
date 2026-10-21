"""Exercise the packaged adapter's lifecycle and target admission boundaries."""
import asyncio
import importlib.util
import json
from pathlib import Path
import sys
import types
import unittest
from unittest.mock import patch


DRIVER = Path(__file__).resolve().parents[3] / 'talos-rs/crates/action/browser/resources/browser_driver.py'


class DriverLifecycleTest(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        spec = importlib.util.spec_from_file_location('driver_lifecycle_fixture', DRIVER)
        self.driver = importlib.util.module_from_spec(spec)
        stdout = sys.stdout
        try:
            spec.loader.exec_module(self.driver)
        finally:
            sys.stdout = stdout
        # Keep the same cancellation semantics with a short deterministic test budget.
        self.driver.START_TIMEOUT_SECONDS = .03
        self.driver.STOP_TIMEOUT_SECONDS = .03
        owner = self
        self.start_mode = self.stop_mode = 'success'
        self.started = asyncio.Event()
        self.start_cancelled = self.stop_cancelled = False
        self.resource_open = False
        self.stop_count = 0
        self.visits = []
        self.targets = ['owned-page']
        self.activations = []
        self.switches = []

        class Switch:
            def __init__(self, target_id):
                self.target_id = target_id
                owner.switches.append(target_id)

            def __await__(self):
                return self.event_result().__await__()

            async def event_result(self, **_):
                if self.target_id not in owner.targets:
                    raise ValueError('Requested tab has closed')
                return self.target_id

        class Page:
            def __init__(self, target_id='owned-page'):
                self._target_id = target_id

            async def goto(self, url):
                owner.visits.append(url)

            async def get_target_info(self):
                return {'targetId': self._target_id, 'url': 'http://page.test', 'title': 'Owned page'}

        class Session:
            def __init__(self, **_):
                async def activate(parameters):
                    owner.activations.append(parameters['targetId'])
                self.cdp_client = types.SimpleNamespace(send=types.SimpleNamespace(
                    Target=types.SimpleNamespace(activateTarget=activate)))
                self.event_bus = types.SimpleNamespace(dispatch=lambda event: event)

            async def start(self):
                owner.resource_open = True
                owner.started.set()
                if owner.start_mode == 'failure':
                    raise RuntimeError('partial initialization failed')
                if owner.start_mode == 'hang':
                    try:
                        await asyncio.Event().wait()
                    finally:
                        owner.start_cancelled = True

            async def must_get_current_page(self):
                return Page()

            async def get_pages(self):
                return [Page(target) for target in owner.targets]

            async def new_page(self, _):
                owner.targets.append('new-page')
                return Page('new-page')

            async def stop(self):
                owner.stop_count += 1
                if owner.stop_mode == 'failure':
                    raise RuntimeError('session cleanup failed')
                if owner.stop_mode == 'hang':
                    try:
                        await asyncio.Event().wait()
                    finally:
                        owner.stop_cancelled = True
                owner.resource_open = False

        browser_use = types.ModuleType('browser_use')
        browser_use.BrowserSession = Session
        browser = types.ModuleType('browser_use.browser')
        events = types.ModuleType('browser_use.browser.events')
        events.SwitchTabEvent = Switch
        self.modules = patch.dict(sys.modules, {
            'browser_use': browser_use,
            'browser_use.browser': browser,
            'browser_use.browser.events': events,
        })
        self.modules.start()
        self.addCleanup(self.modules.stop)
        self.request = {'cdp': 'ws://owned.test/private', 'downloads': '/owned/downloads',
                        'action': {'kind': 'navigate', 'url': 'http://page.test'}}

    async def test_partial_start_failure_still_releases_session(self):
        self.start_mode = 'failure'
        with self.assertRaisesRegex(RuntimeError, 'partial initialization failed'):
            await self.driver.act(self.request)
        self.assertEqual(self.stop_count, 1)
        self.assertFalse(self.resource_open)
        self.assertEqual(self.visits, [])

    async def test_hanging_start_is_cancelled_then_cleaned(self):
        self.start_mode = 'hang'
        with self.assertRaises(asyncio.TimeoutError):
            await self.driver.act(self.request)
        self.assertTrue(self.start_cancelled)
        self.assertEqual(self.stop_count, 1)
        self.assertFalse(self.resource_open)
        self.assertEqual(self.visits, [])

    async def test_caller_cancellation_still_cleans_partial_session(self):
        self.start_mode = 'hang'
        task = asyncio.create_task(self.driver.act(self.request))
        await self.started.wait()
        task.cancel()
        with self.assertRaises(asyncio.CancelledError):
            await task
        self.assertTrue(self.start_cancelled)
        self.assertEqual(self.stop_count, 1)
        self.assertFalse(self.resource_open)

    async def test_normal_success_closes_session_before_returning(self):
        result = await self.driver.act({**self.request, 'targetId': 'owned-page'})
        self.assertEqual(result['page']['targetId'], 'owned-page')
        self.assertEqual(self.visits, ['http://page.test'])
        self.assertEqual(self.stop_count, 1)
        self.assertFalse(self.resource_open)

    async def test_stop_failure_cannot_return_operation_success(self):
        self.stop_mode = 'failure'
        with self.assertRaisesRegex(RuntimeError, 'session cleanup failed'):
            await self.driver.act(self.request)
        self.assertEqual(self.stop_count, 1)

    async def test_hanging_stop_fails_explicitly_and_is_cancelled(self):
        self.stop_mode = 'hang'
        with self.assertRaises(asyncio.TimeoutError):
            await self.driver.act(self.request)
        self.assertTrue(self.stop_cancelled)
        self.assertEqual(self.stop_count, 1)

    async def test_actions_reject_closed_target_before_dispatch(self):
        for kind in ('navigate', 'click', 'fill', 'upload', 'key', 'scroll'):
            with self.subTest(kind=kind):
                request = {**self.request, 'targetId': 'closed-page', 'action': {'kind': kind}}
                with self.assertRaises(self.driver.StaleElement):
                    await self.driver.act(request)
        self.assertEqual(self.visits, [])
        self.assertEqual(self.activations, [])
        self.assertEqual(self.stop_count, 6)

    async def test_explicit_new_tab_recovers_from_closed_current_target(self):
        request = {**self.request, 'targetId': 'closed-page',
                   'action': {'kind': 'tab', 'action': 'open', 'url': 'http://page.test'}}
        result = await self.driver.act(request)
        self.assertEqual(result['page']['targetId'], 'new-page')
        self.assertEqual(self.activations, ['new-page'])
        self.assertEqual(self.stop_count, 1)

    async def test_action_rejects_unexpected_focus_recovery_before_input(self):
        self.targets = ['requested-page']
        with self.assertRaises(self.driver.StaleElement):
            await self.driver.act({**self.request, 'targetId': 'requested-page'})
        self.assertEqual(self.visits, [])
        self.assertEqual(self.stop_count, 1)

    async def test_explicit_switch_propagates_closed_requested_tab_failure(self):
        request = {**self.request, 'targetId': 'owned-page',
                   'action': {'kind': 'tab', 'action': 'focus', 'tabId': 'closed-page'}}
        with self.assertRaisesRegex(ValueError, 'Requested tab has closed'):
            await self.driver.act(request)
        self.assertEqual(self.switches, ['closed-page'])
        self.assertEqual(self.activations, [])
        self.assertEqual(self.visits, [])
        self.assertEqual(self.stop_count, 1)

    async def test_explicit_switch_recovers_closed_current_tab_to_requested_live_tab(self):
        request = {**self.request, 'targetId': 'closed-page',
                   'action': {'kind': 'tab', 'action': 'focus', 'tabId': 'owned-page'}}
        result = await self.driver.act(request)
        self.assertEqual(result['page']['targetId'], 'owned-page')
        self.assertEqual(self.switches, ['owned-page'])
        self.assertEqual(self.stop_count, 1)

    async def observe(self, action, target, targets):
        """Drive the real CDP framing against a controlled loopback transport."""
        self.cdp_commands = []

        async def respond(socket):
            async for raw in socket:
                request = json.loads(raw)
                self.cdp_commands.append(request)
                method = request['method']
                responses = {
                    'Target.getTargets': {'targetInfos': targets},
                    'Target.attachToTarget': {'sessionId': 'owned-session'},
                    'Page.getFrameTree': {'frameTree': {'frame': {'id': 'owned-frame', 'loaderId': 'loader'}}},
                    'DOMSnapshot.captureSnapshot': {'strings': [], 'documents': []},
                    'Page.captureScreenshot': {'data': 'owned-image-bytes'},
                }
                await socket.send(json.dumps({'id': request['id'], 'result': responses[method]}))

        async with self.driver.websockets.serve(respond, '127.0.0.1', 0) as server:
            endpoint = f'ws://127.0.0.1:{server.sockets[0].getsockname()[1]}'
            return await self.driver.observe({'cdp': endpoint, 'targetId': target, 'action': {'kind': action}})

    async def test_screenshot_rejects_closed_target_without_attaching_another_tab(self):
        targets = [{'type': 'page', 'targetId': 'other-page', 'url': 'http://other.test'}]
        with self.assertRaises(self.driver.StaleElement):
            await self.observe('screenshot', 'closed-page', targets)
        self.assertEqual([call['method'] for call in self.cdp_commands], ['Target.getTargets'])

    async def test_state_explicitly_recovers_after_current_tab_closed(self):
        targets = [{'type': 'page', 'targetId': 'other-page', 'url': 'http://other.test'}]
        result = await self.observe('state', 'closed-page', targets)
        self.assertEqual(result['page']['targetId'], 'other-page')
        attach = next(call for call in self.cdp_commands if call['method'] == 'Target.attachToTarget')
        self.assertEqual(attach['params']['targetId'], 'other-page')

    async def test_screenshot_keeps_selected_target_when_it_is_not_first(self):
        targets = [{'type': 'page', 'targetId': target, 'url': 'http://page.test'}
                   for target in ('other-page', 'owned-page')]
        result = await self.observe('screenshot', 'owned-page', targets)
        self.assertEqual(result['page']['targetId'], 'owned-page')
        self.assertEqual(result['screenshot'], 'owned-image-bytes')
        attach = next(call for call in self.cdp_commands if call['method'] == 'Target.attachToTarget')
        self.assertEqual(attach['params']['targetId'], 'owned-page')


if __name__ == '__main__':
    unittest.main()
