"""Reuse the production Desktop harness for generic files and client-specific link routing."""
import sys

from preview_fixture import PreviewCluster
from desktop_resources import run, native_pages, invoke
from fixture import REPORT, require, wait_for
from format_matrix import FIXTURES, exercise_formats
from web import HTML, PNG
from playwright.sync_api import expect


def exercise(page, token, destination, markdown, endpoint):
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
    links.write_text('[Default browser](' + destination + ')\n')
    page.locator('input[type=file]').set_input_files(str(links))
    expect(page.get_by_role('heading', name=links.name, exact=True)).to_be_visible()
    page.get_by_role('button', name='Open file', exact=True).click()
    expect(workspace.get_by_role('link', name='Default browser', exact=True)).to_be_visible()
    count = workspace.get_by_role('tab').count()
    workspace.get_by_role('link', name='Default browser', exact=True).click()
    expect(workspace.get_by_role('tab')).to_have_count(count)
    # Real preload/main dispatches the URL. OS default-app rendering is outside
    # this harness; deterministic URL/policy branches live in frontend units.
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
    def upload_format(case):
        page.locator('input[type=file]').set_input_files(str(FIXTURES / case['name']))
        expect(page.get_by_role('heading', name=case['name'], exact=True)).to_be_visible()
        return next(file for file in invoke(page, token, 'file/list', {'limit': 50})['data'] if file['originalFilename'] == case['name'])
    formats = exercise_formats(page, upload_format, native_pages=native_pages)
    return {'formatsVerified': len(formats), 'genericHtml': True, 'filesImageRightPane': True,
            'desktopLinkDoesNotCreateTab': True, 'osDefaultApplicationRenderingVerified': False}


if __name__ == '__main__':
    cluster = PreviewCluster()
    try:
        run(cluster, exercise_resources=exercise)
    finally:
        cluster.close()
