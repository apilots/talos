"""Reuse the production Desktop harness for generic files and client-specific link routing."""
from preview_fixture import PreviewCluster
from desktop_resources import run, exercise as exercise_workspace
from fixture import REPORT, require
from web import HTML, PNG
from playwright.sync_api import expect


def exercise(page, token, destination, markdown):
    evidence = exercise_workspace(page, token, destination, markdown)
    workspace = page.get_by_role('dialog', name='Right workspace', exact=True)
    home = markdown.parent
    html = home / 'native-report.html'
    html.write_bytes(HTML)
    page.locator('input[type=file]').set_input_files(str(html))
    expect(page.get_by_role('heading', name=html.name, exact=True)).to_be_visible()
    page.get_by_role('button', name='Preview file', exact=True).click()
    frame = workspace.locator('iframe[title="native-report.html"]').element_handle()
    expect(frame.content_frame().get_by_role('heading', name='Resource report')).to_be_visible()
    require(frame.content_frame().evaluate('typeof window.talos') == 'undefined', 'Registered HTML inherited the Desktop bridge')
    frame.content_frame().get_by_role('textbox', name='Report draft').fill('Native registered HTML')
    image = home / 'native-pixel.png'
    image.write_bytes(PNG)
    page.locator('input[type=file]').set_input_files(str(image))
    expect(page.get_by_role('heading', name=image.name, exact=True)).to_be_visible()
    page.get_by_role('button', name='Preview file', exact=True).click()
    expect(workspace.locator('iframe[title="native-pixel.png"]')).to_be_visible()
    expect(page.get_by_role('dialog', name='Preview native-pixel.png', exact=True)).to_have_count(0)
    workspace.get_by_role('tab', name='native-report.html', exact=True).click()
    expect(frame.content_frame().get_by_role('textbox', name='Report draft')).to_have_value('Native registered HTML')
    links = home / 'native-links.md'
    links.write_text('[Default browser](' + destination + ')\n')
    page.locator('input[type=file]').set_input_files(str(links))
    expect(page.get_by_role('heading', name=links.name, exact=True)).to_be_visible()
    page.get_by_role('button', name='Preview file', exact=True).click()
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
    evidence.update(genericHtml=True, filesImageRightPane=True, desktopLinkDoesNotCreateTab=True,
                    osDefaultApplicationRenderingVerified=False)
    return evidence


if __name__ == '__main__':
    cluster = PreviewCluster()
    try:
        run(cluster, exercise_resources=exercise)
    finally:
        cluster.close()
