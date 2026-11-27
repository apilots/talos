"""Exercise sidebar pinning against the real Server while its Turn is running."""
import sqlite3
from urllib.parse import urlsplit

from playwright.sync_api import expect


def verify_session_pinning(page, origin, database, session_id, project_name, report):
    def record():
        with sqlite3.connect(f'file:{database}?mode=ro', uri=True) as db:
            row = db.execute(
                'SELECT pinned, last_activity FROM sessions WHERE session_key = ?',
                (session_id,),
            ).fetchone()
            active = db.execute(
                'SELECT turn_id FROM agent_active_turns WHERE session_id = ?',
                (session_id,),
            ).fetchone()
        assert row is not None and active is not None, 'Acceptance requires a running Turn'
        return row

    def patch_response(response):
        return response.request.method == 'PATCH' and urlsplit(response.url).path == f'/v1/sessions/{session_id}'

    def toggle(pinned, collapsed_destination=False):
        button = page.locator(f'[data-sidebar-pin-button="{session_id}"]')
        expect(button).to_have_count(1)
        button.focus()
        expect(button).to_be_focused()
        current_url = page.url
        with page.expect_response(patch_response) as patched:
            button.press('Enter')
        assert patched.value.ok, patched.value.text()
        assert page.url == current_url, 'Pinning must not select the Session'
        assert record() == (int(pinned), activity), 'Pinning must preserve activity during a Turn'
        expect(page.get_by_role('region', name='Pinned', exact=True)).to_have_count(int(pinned))
        if collapsed_destination:
            expect(button).to_have_count(0)
            expect(page.get_by_role('button', name=project_name, exact=True)).to_be_focused()
            return
        expect(button).to_have_attribute('aria-pressed', str(pinned).lower())
        expect(button).to_be_focused()
        expect(button).not_to_have_attribute('aria-disabled', 'true')
        expect(button).to_be_visible()
        page.wait_for_function(
            '(id) => Number(getComputedStyle(document.querySelector(`[data-sidebar-pin-button="${id}"]`).parentElement).opacity) === 1',
            arg=session_id,
        )

    initially_pinned, activity = record()
    assert not initially_pinned
    page.goto(origin)
    page.get_by_role('button', name='Expand project', exact=True).click()
    toggle(True)
    page.screenshot(path=str(report / 'session-pinned-running.png'), full_page=True)
    page.reload()
    button = page.get_by_role('region', name='Pinned', exact=True).locator(
        f'[data-sidebar-pin-button="{session_id}"]',
    )
    expect(button).to_have_attribute('aria-pressed', 'true')
    assert record() == (1, activity)
    toggle(False, collapsed_destination=True)
    page.screenshot(path=str(report / 'session-unpin-collapsed-project.png'), full_page=True)
    page.get_by_role('button', name='Expand project', exact=True).click()

    # A rejected write must roll back placement and leave the control usable.
    pattern = f'**/v1/sessions/{session_id}'

    def reject(route):
        if route.request.method == 'PATCH':
            route.fulfill(status=503, content_type='application/json', body='{}')
        else:
            route.continue_()

    page.route(pattern, reject)
    try:
        button = page.locator(f'[data-sidebar-pin-button="{session_id}"]')
        button.focus()
        button.press('Enter')
        expect(page.get_by_role('alert')).to_be_visible()
        expect(button).to_have_attribute('aria-pressed', 'false')
        expect(button).not_to_have_attribute('aria-disabled', 'true')
        expect(button).to_be_focused()
        expect(page.get_by_role('region', name='Pinned', exact=True)).to_have_count(0)
        assert record() == (0, activity)
        page.screenshot(path=str(report / 'session-pin-rejected.png'), full_page=True)
    finally:
        page.unroute(pattern, reject)

    toggle(True)
    expect(page.get_by_role('alert')).to_have_count(0)
    toggle(False)
    page.goto(origin + '/sessions/' + session_id)
    print('CASE session_pinning PASS server=real database=SQLite running=pin,unpin focus=restored refresh=persisted rejection=rollback,retry activity=preserved')
    return {'sessionId': session_id, 'runningTurn': True, 'focusRestored': True,
            'refreshPersisted': True, 'rejectionRecovered': True, 'activityPreserved': True,
            'collapsedDestinationFocus': True}
