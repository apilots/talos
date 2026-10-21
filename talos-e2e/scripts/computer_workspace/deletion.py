"""Disposable Computer deletion acceptance against real Cloud Auth and Talos HTTP/RPC."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import urllib.error
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'multi_node'))
from fixture import Cluster, ROOT, REPORT, data, free_port, require, wait_for
from playwright.sync_api import sync_playwright, expect


def public(cluster, path, method='GET', body=None, user='alice'):
    request = urllib.request.Request(cluster.url + path, method=method,
        data=None if body is None else json.dumps(body).encode(),
        headers={'Content-Type': 'application/json', 'Authorization': 'Bearer ' + cluster.users[user]['access_token']})
    try:
        response = urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request, timeout=20)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return json.load(response)


def run(cluster):
    cluster.start_control_plane()
    records = []
    for name in ('开发笔记本', '旧测试工作站'):
        records.append(data(public(cluster, '/v1/computers', 'POST', {'displayName': name, 'credentialExpiresAt': 2_000_000_000})))
    old = records[1]['node']['nodeId']
    path = '/v1/computers/' + old
    require(public(cluster, path, 'DELETE', user='bob')['error']['code'] == 'resourceNotFound', 'Non-owner deletion was accepted')
    port = free_port()
    env = dict(os.environ, VITE_API_PROXY_TARGET=cluster.url)
    cluster.start_process('web', ['pnpm', '--dir', str(ROOT / 'talos-app'), '--filter', 'talos-web', 'dev', '--port', str(port)], env)
    url = f'http://127.0.0.1:{port}'
    wait_for(lambda: urllib.request.build_opener(urllib.request.ProxyHandler({})).open(url, timeout=2).status == 200, 'Web development server')
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path=os.environ.get('TALOS_E2E_CHROME_BIN', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'), headless=True)
        context = browser.new_context(viewport={'width': 1560, 'height': 1050}, locale='zh-CN', timezone_id='Asia/Shanghai')
        context.add_init_script('sessionStorage.setItem("talos-web-auth-token", ' + json.dumps(cluster.users['alice']['access_token']) + ');')
        # Seed before the store mounts: a late auth-state persistence write must
        # not overwrite a locale injected into an already running page.
        context.add_init_script('if (!localStorage.getItem("talos-web-ui")) localStorage.setItem("talos-web-ui", JSON.stringify({state:{locale:"zh-CN",uiTheme:"light"},version:0}));')
        page = context.new_page()
        page.goto(url + '/computers')
        expect(page.locator('html')).to_have_attribute('lang', 'zh-CN')
        page.locator('.cmp-content').get_by_role('button', name=re.compile('旧测试工作站')).first.click()
        expect(page.locator('.cmp-inspector h2')).to_have_text('旧测试工作站')
        page.get_by_role('button', name='进入电脑', exact=True).click()
        page.goto(url + '/computers')
        page.locator('.cmp-content').get_by_role('button', name=re.compile('旧测试工作站')).first.click()
        actions = page.locator('.cmp-registration-actions')
        expect(actions.get_by_role('button', name='停用', exact=True)).to_be_visible()
        expect(actions.get_by_role('button', name='移除注册', exact=True)).to_be_visible()
        rects = actions.locator('button').evaluate_all('(buttons)=>buttons.map(b=>{const r=b.getBoundingClientRect();return {top:r.top,left:r.left,width:r.width}})')
        require(abs(rects[0]['top'] - rects[1]['top']) < 2 and rects[1]['left'] > rects[0]['left'], 'Bottom actions are not side by side')
        actions.scroll_into_view_if_needed()
        page.screenshot(path=str(REPORT / 'computers-actions-zh.png'), full_page=True)
        page.locator('.cmp-inspector').screenshot(path=str(REPORT / 'computer-details-zh.png'))
        actions.get_by_role('button', name='停用', exact=True).click()
        expect(actions.get_by_role('button', name='启用', exact=True)).to_be_visible()
        require(data(public(cluster, path))['status'] == 'disabled', 'Disable did not reach the real Service')
        actions.get_by_role('button', name='启用', exact=True).click()
        expect(actions.get_by_role('button', name='停用', exact=True)).to_be_visible()
        actions.get_by_role('button', name='移除注册', exact=True).click()
        dialog = page.get_by_role('dialog')
        expect(dialog).to_contain_text('工作区文件均会保留')
        page.screenshot(path=str(REPORT / 'computers-confirm-zh.png'), full_page=True)
        dialog.get_by_role('button', name='取消', exact=True).click()
        require(data(public(cluster, '/v1/computers'))['total'] == 2, 'Cancel changed registration')
        page.evaluate('const s=JSON.parse(localStorage.getItem("talos-web-ui"));s.state.locale="en";localStorage.setItem("talos-web-ui",JSON.stringify(s))')
        page.reload()
        page.locator('.cmp-content').get_by_role('button', name=re.compile('旧测试工作站')).first.click()
        page.locator('.cmp-registration-actions').get_by_role('button', name='Remove registration', exact=True).click()
        expect(page.get_by_role('dialog')).to_contain_text('Projects, sessions, history and Workspace files are kept')
        page.screenshot(path=str(REPORT / 'computers-confirm-en.png'), full_page=True)
        page.get_by_role('dialog').get_by_role('button', name='Remove registration', exact=True).click()
        expect(page.get_by_role('dialog')).to_have_count(0)
        expect(page.locator('.cmp-content')).not_to_contain_text('旧测试工作站')
        require(data(public(cluster, '/v1/computers'))['total'] == 1, 'Successful deletion left an authoritative directory entry')
        page.screenshot(path=str(REPORT / 'computers-removed-en.png'), full_page=True)
        saved = page.evaluate('JSON.parse(localStorage.getItem("talos-web-ui")).state.selectedComputers')
        require('' in saved.values() and old not in saved.values(), 'Deleted selection redirected or remained selected')
        rejected = cluster.rpc('AgentNodeControl', 'Register', {'auth': {'node_id': old, 'token': records[1]['credential']['token']}, 'params': {'deployments': [{'agent_id': 'default'}]}})
        require(rejected.get('failure', {}).get('error', {}).get('code') == 'ERROR_CODE_UNAUTHENTICATED', 'Deleted registration credential was accepted')
        require(data(public(cluster, path, 'DELETE'))['nodeId'] == old, 'Deletion retry is not idempotent')

        # Clearing a removed selection must lead to a usable homepage, not an
        # empty Projects/Chats sidebar. The remaining registered Computer is offline.
        page.goto(url + '/')
        expect(page.get_by_role('heading', name='Choose a computer', exact=True)).to_be_visible()
        expect(page.get_by_role('banner')).to_have_count(0)
        require(page.get_by_role('main').bounding_box()['y'] <= 1, 'Hidden homepage header left empty space')
        expect(page.locator('#talos-sidebar')).to_have_count(0)
        expect(page.get_by_role('link', name='New chat', exact=True)).to_have_count(0)
        page.screenshot(path=str(REPORT / 'homepage-choose-computer.png'), full_page=True)
        page.locator('main').get_by_role('button', name=re.compile('开发笔记本')).click()
        expect(page.locator('#talos-sidebar')).to_be_visible()
        expect(page.get_by_role('heading', name='Start with one clear prompt')).to_be_visible()
        remaining = records[0]['node']['nodeId']
        require(data(public(cluster, '/v1/computers/' + remaining, 'DELETE'))['nodeId'] == remaining,
                'Final disposable Computer was not removed')
        expect(page.get_by_role('heading', name='Connect your first computer')).to_be_visible(timeout=15000)
        expect(page.locator('#talos-sidebar')).to_have_count(0)
        for width in (1560, 390):
            page.set_viewport_size({'width': width, 'height': 900})
            page.reload()
            expect(page.get_by_role('heading', name='Connect your first computer')).to_be_visible()
            expect(page.get_by_role('banner')).to_have_count(0)
            require(page.get_by_role('main').bounding_box()['y'] <= 1, 'Empty directory retained a header spacer')
            expect(page.get_by_role('dialog')).to_have_count(0)
            trigger = page.get_by_role('button', name='Connect a computer', exact=True)
            trigger.click()
            expect(page.get_by_role('dialog', name='Connect a computer')).to_contain_text('same account')
            if width == 1560:
                page.screenshot(path=str(REPORT / 'homepage-connect-guide.png'), full_page=True)
            page.keyboard.press('Escape')
            expect(page.get_by_role('dialog')).to_have_count(0)
            expect(trigger).to_be_focused()
        page.screenshot(path=str(REPORT / 'homepage-no-computers.png'), full_page=True)
        print('CASE computer_home_selection_empty_directory_and_connection_guide PASS', flush=True)

        # Search must discover a Computer outside the initial directory response.
        registered = [data(public(cluster, '/v1/computers', 'POST', {
            'displayName': f'Homepage Worker {index:02}', 'credentialExpiresAt': 2_000_000_000,
        }))['node'] for index in range(1, 26)]
        initial = data(public(cluster, '/v1/computers?limit=20'))
        loaded = {node['nodeId'] for node in initial['data']}
        target = next(node for node in registered if node['nodeId'] not in loaded)
        page.set_viewport_size({'width': 1560, 'height': 900})
        page.reload()
        search = page.get_by_role('searchbox', name='Search computers')
        expect(search).to_be_visible()
        rows = page.locator('main [data-computer-id]')
        expect(rows).to_have_count(len(initial['data']))
        expect(page.get_by_role('button', name='Show more', exact=True)).to_have_count(0)
        require(rows.first.evaluate('(row) => row.parentElement.scrollHeight > row.parentElement.clientHeight'),
                'Long homepage list does not scroll internally')
        page.screenshot(path=str(REPORT / 'homepage-long-list.png'), full_page=True)
        search.fill(target['displayName'])
        expect(rows).to_have_count(1)
        expect(page.locator(f'main [data-computer-id="{target["nodeId"]}"]')).to_be_visible()
        page.screenshot(path=str(REPORT / 'homepage-search-match.png'), full_page=True)
        search.fill('no-such-homepage-computer')
        expect(page.locator('main').get_by_text('No computers found', exact=True)).to_be_visible()
        expect(page.get_by_role('heading', name='Choose a computer', exact=True)).to_be_visible()
        search.fill('')
        expect(rows).to_have_count(len(initial['data']))
        print('CASE computer_home_long_list_server_search_and_clear PASS', flush=True)
        browser.close()
    print('CASE computer_delete_owner_offline_status_cancel_confirm_i18n_selection_credentials PASS', flush=True)


if __name__ == '__main__':
    if '--contract-auth' in sys.argv:
        from deletion_auth_fixture import ContractAuthCluster
        cluster = ContractAuthCluster()
        print('DETAIL auth=explicit-contract-fixture; Computer Gateway/Service/Store are real', flush=True)
    else:
        cluster = Cluster()
        print('DETAIL auth=real-isolated-cloud', flush=True)
    try:
        run(cluster)
    finally:
        cluster.close()
