"""Reuse the production Desktop harness for generic files and client-specific link routing."""
import subprocess
import sys
from uuid import uuid4

from preview_fixture import ModelReply, PreviewCluster
from desktop_resources import run, native_pages, invoke
from fixture import REPORT, data, require, wait_for
from format_matrix import FIXTURES, exercise_formats
from web import HTML, PNG
from playwright.sync_api import expect


def exercise_worktree(page, token, home, model):
    checkout = home.resolve() / 'checkout'
    worktree = home.resolve() / 'feature-worktree'
    subprocess.run(['git', 'init', '--quiet', str(checkout)], check=True)
    subprocess.run(['git', '-C', str(checkout), '-c', 'user.name=Talos Test',
                    '-c', 'user.email=talos@example.test', '-c', 'commit.gpgsign=false',
                    '-c', 'core.hooksPath=/dev/null', 'commit', '--quiet',
                    '--allow-empty', '-m', 'Initial commit'], check=True)
    subprocess.run(['git', '-C', str(checkout), 'worktree', 'add', '--quiet',
                    '--detach', str(worktree)], check=True)
    plan = worktree / 'IMPLEMENTATION_PLAN.md'
    plan.write_text('# Worktree implementation plan\n')
    notes = worktree / 'NOTES.md'
    notes.write_text('# Worktree notes\n')
    outside = checkout / 'outside.md'
    outside.write_text('# Outside the worktree\n')
    identity = data(page.evaluate('async () => window.talos.workerIdentity()'))
    node = identity['nodeId']
    project = invoke(page, token, 'project/create', {
        'nodeId': node, 'name': 'Worktree preview project', 'directory': str(checkout),
    })['project']
    session = f'sess_{uuid4().hex}'
    ModelReply.text = f'Inspect `NOTES.md` or [Worktree plan](<{plan}>)\n\n[Outside file](<{outside}>)'
    invoke(page, token, 'turn/start', {
        'nodeId': node, 'sessionId': session, 'projectId': project['project_id'],
        'currentDirectory': None, 'clientRequestId': str(uuid4()),
        'modelSelection': {'modelRef': model, 'reasoningEffort': None},
        'input': {'content': [{'type': 'text', 'text': f'Bind existing worktree `{worktree}`, then open the worktree plan.'}]},
    })
    page.goto(page.url.split('#', 1)[0] + '#/sessions/' + session)
    workspace = page.get_by_role('dialog', name='Right workspace', exact=True)
    page.get_by_role('link', name='NOTES.md', exact=True).click(timeout=45_000)
    expect(workspace.get_by_role('heading', name='Worktree notes')).to_be_visible()
    expect(workspace.get_by_role('textbox', name='File location', exact=True)).to_have_value('NOTES.md')
    page.get_by_role('link', name='Worktree plan', exact=True).click()
    expect(workspace.get_by_role('heading', name='Worktree implementation plan')).to_be_visible()
    expect(workspace.get_by_role('textbox', name='File location', exact=True)).to_have_value(str(plan))
    page.screenshot(path=str(REPORT / 'native-worktree-preview.png'))
    page.get_by_role('link', name='Outside file', exact=True).click()
    expect(workspace.get_by_role('alert')).to_have_text('authorization denied')
    page.screenshot(path=str(REPORT / 'native-worktree-outside-denied.png'))
    subprocess.run(['git', '-C', str(checkout), 'worktree', 'remove', '--force', str(worktree)], check=True)
    workspace.get_by_role('tab', name=plan.name, exact=True).click()
    workspace.get_by_role('button', name='Refresh webpage', exact=True).click()
    expect(workspace.get_by_role('alert')).to_contain_text('Current working directory missing')
    page.screenshot(path=str(REPORT / 'native-worktree-missing.png'))


def exercise(page, token, destination, markdown, endpoint, *, model):
    expect(page.get_by_role('button', name='Upload', exact=True)).to_be_visible(timeout=45_000)
    workspace = page.get_by_role('dialog', name='Right workspace', exact=True)
    home = markdown.parent
    html = home / 'native-report.html'
    html.write_bytes(HTML)
    page.locator('input[type=file]').set_input_files(str(html))
    expect(page.get_by_role('heading', name=html.name, exact=True)).to_be_visible()
    page.get_by_role('button', name='Open file', exact=True).click()
    frame = wait_for(lambda: next((target for target in native_pages(page) if target.url.startswith('talos-preview:') and target.title() == 'Resource report'), None), 'Native HTML protocol target')
    expect(frame.get_by_role('heading', name='Resource report')).to_be_visible()
    require(frame.evaluate('typeof window.talos') == 'undefined', 'Registered HTML inherited the Desktop bridge')
    frame.get_by_role('textbox', name='Report draft').fill('Native registered HTML')
    image = home / 'native-pixel.png'
    image.write_bytes(PNG)
    page.locator('input[type=file]').set_input_files(str(image))
    expect(page.get_by_role('heading', name=image.name, exact=True)).to_be_visible()
    page.get_by_role('button', name='Open file', exact=True).click()
    expect(workspace.get_by_role('region', name='native-pixel.png', exact=True)).to_be_visible()
    expect(page.get_by_role('dialog', name='Preview native-pixel.png', exact=True)).to_have_count(0)
    workspace.get_by_role('tab', name='native-report.html', exact=True).click()
    expect(frame.get_by_role('textbox', name='Report draft')).to_have_value('Native registered HTML')
    links = home / 'native-links.md'
    links.write_text('[Embedded browser](' + destination + ')\n')
    page.locator('input[type=file]').set_input_files(str(links))
    expect(page.get_by_role('heading', name=links.name, exact=True)).to_be_visible()
    page.get_by_role('button', name='Open file', exact=True).click()
    expect(workspace.get_by_role('link', name='Embedded browser', exact=True)).to_be_visible()
    count = workspace.get_by_role('tab').count()
    workspace.get_by_role('link', name='Embedded browser', exact=True).click()
    expect(workspace.get_by_role('tab')).to_have_count(count + 1)
    expect(workspace.get_by_role('textbox', name='Web address', exact=True)).to_have_value(destination)
    expect(workspace.get_by_role('alert')).to_have_count(0)
    workspace.get_by_role('button', name='Open in options', exact=True).click()
    expect(page.get_by_role('menuitem', name='Talos', exact=True)).to_be_visible()
    page.keyboard.press('Escape')
    page.screenshot(path=str(REPORT / 'native-unified-preview.png'), full_page=True)
    workspace.get_by_role('tab', name='native-report.html', exact=True).click()
    frame.screenshot(path=str(REPORT / 'native-html-content.png'))
    workspace.get_by_role('button', name='Open in options', exact=True).click()
    expect(page.get_by_role('menuitem', name='Talos', exact=True)).to_be_visible()
    if sys.platform == 'darwin':
        # Safari ships with macOS. Discover the real app; mocks cannot prove the
        # native helper survived bundling/packaging.
        expect(page.get_by_role('menuitem', name='Safari', exact=True)).to_be_visible()
    page.screenshot(path=str(REPORT / 'native-target-menu.png'), full_page=True)
    page.keyboard.press('Escape')
    exercise_worktree(page, token, home, model)
    page.goto(page.url.split('#', 1)[0] + '#/files')
    expect(page.get_by_role('button', name='Upload', exact=True)).to_be_visible(timeout=45_000)
    def upload_format(case):
        page.locator('input[type=file]').set_input_files(str(FIXTURES / case['name']))
        expect(page.get_by_role('heading', name=case['name'], exact=True)).to_be_visible()
        return next(file for file in invoke(page, token, 'file/list', {'limit': 50})['data'] if file['originalFilename'] == case['name'])
    formats = exercise_formats(page, upload_format, native_pages=native_pages)
    return {'formatsVerified': len(formats), 'genericHtml': True, 'filesImageRightPane': True,
            'desktopLinkOpensEmbeddedTab': True, 'osDefaultApplicationRenderingVerified': False,
            'createdTurn': True, 'worktreePreview': True, 'outsideWorktreeDenied': True,
            'missingWorktreeReported': True}


if __name__ == '__main__':
    cluster = PreviewCluster()
    try:
        run(cluster, exercise_resources=lambda *args: exercise(*args, model=cluster.model))
    finally:
        cluster.close()
