"""Capture owner-visible product pages from the running authenticated E2E topology."""
import json
import os
from pathlib import Path
import subprocess
import uuid
from cluster import REPORT, ROOT, free_port, require, wait_for
from turn_helpers import submit, settled


def capture(cluster):
    from ui import Driver
    os.environ.setdefault('TALOS_E2E_CHROME_BIN', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome')
    output = REPORT / 'web-dist'
    with (REPORT / 'web-build.log').open('w') as log:
        subprocess.run(['pnpm', '--dir', str(ROOT / 'talos-app/web'), 'exec', 'vite', 'build',
                        '--outDir', str(output), '--emptyOutDir'],
                       env=dict(os.environ, VITE_API_BASE_URL=cluster.url, VITE_TALOS_DESKTOP='0'),
                       stdout=log, stderr=subprocess.STDOUT, check=True)
    port = free_port()
    cluster.start_process('review-web', ['python3', str(Path(__file__).resolve().parents[1] / 'computer_workspace/web_static.py'), str(port), str(output)])
    url = f'http://127.0.0.1:{port}'
    driver = Driver(cluster)
    try:
        driver.call('web', 'launch', name='web', port=free_port(), home=str(cluster.root / 'review-chrome'),
                    config='', resourceDir=str(ROOT), report=str(REPORT), spki='', webUrl=url, headless=True)
        driver.call('web', 'login', ingress=cluster.url, cloud=cluster.cloud_url, token=cluster.users['alice']['access_token'])
        driver.call('web', 'viewport', width=1440, height=960)
        def navigate(path):
            driver.call('web', 'evaluate', expression=f'history.pushState(null,"",{json.dumps(path)});window.dispatchEvent(new PopStateEvent("popstate"))')
        navigate('/computers')
        docker = cluster.nodes['Docker-Worker']['node_id']
        driver.call('web', 'wait', expression=f'Boolean(document.querySelector(\'main [data-computer-id="{docker}"]\'))')
        driver.call('web', 'evaluate', expression=f'document.querySelector(\'main [data-computer-id="{docker}"]\').click()')
        driver.call('web', 'wait', expression='document.querySelector("main").innerText.includes("Linux")')
        driver.call('web', 'screenshot', path=str(REPORT / 'owner-computers.png'))
        # Opening a Computer selects its original environment for all business APIs.
        driver.click('web', 'Open computer')
        driver.call('web', 'wait', expression=f"""location.pathname === \"/\" && Boolean(document.querySelector('aside [data-computer-id=\"{docker}\"]'))""")
        navigate('/settings/models')
        driver.call('web', 'wait', expression='Boolean(document.querySelector("main input")) && document.querySelector("main").innerText.includes("DeepSeek")')
        driver.click('web', 'DeepSeek')
        driver.call('web', 'wait', expression='document.querySelector("main").innerText.includes("OWNER_CONFIG_")')
        driver.call('web', 'screenshot', path=str(REPORT / 'owner-model-settings.png'))
        navigate('/settings/web-search')
        driver.call('web', 'wait', expression='location.pathname === "/settings/web-search" && Boolean(document.querySelector("main input"))')
        driver.click('web', 'Brave')
        driver.call('web', 'wait', expression='Array.from(document.querySelectorAll("main input")).some(i=>i.value==="https://search.example.test")')
        driver.call('web', 'screenshot', path=str(REPORT / 'owner-web-search.png'))
        session = str(uuid.uuid4())
        first = submit(cluster, 'Desktop-A', docker, session, 'Use Bash to write SYNC_UI_A to sync-ui-proof.txt. Reply SYNC_UI_A.')
        wait_for(lambda: settled(cluster, 'Desktop-A', session, first['turnId']), 'Screenshot actual first Turn', 120)
        wait_for(lambda: bool(cluster.history('Desktop-B', session)['data']), 'Screenshot imported first Turn')
        second = submit(cluster, 'Desktop-B', docker, session, 'Use Bash to read sync-ui-proof.txt, then append SYNC_UI_B to it. Reply SYNC_UI_B.')
        wait_for(lambda: settled(cluster, 'Desktop-B', session, second['turnId']), 'Screenshot actual continuation', 120)
        wait_for(lambda: len(cluster.history('G', session)['data']) == 2, 'Screenshot center facts synchronized')
        navigate('/sessions/' + session)
        driver.wait_final('web', 'SYNC_UI_B')
        require(driver.call('web', 'evaluate', expression=f'Boolean(document.querySelector(\'[data-computer-id="{docker}"][aria-pressed=true]\'))'), 'History UI lost its immutable Computer')
        driver.call('web', 'screenshot', path=str(REPORT / 'cross-control-plane-history.png'))
        # Reopen the same saved management home in the actual Electron product;
        # never run two embedded Workers for the same persisted identity.
        desktop = cluster.desktops['Desktop-A']
        desktop['host'].close()
        driver.call('desktop', 'launch', name='desktop', port=free_port(), mainPort=free_port(),
                    home=str(desktop['home']), config=str(desktop['home'] / 'conf/talos.yaml'),
                    resourceDir=str(ROOT), report=str(REPORT), spki='')
        local = desktop['node']
        driver.call('desktop', 'wait', expression=f'Boolean(document.querySelector(\'aside [data-computer-id="{local}"][aria-pressed=true]\'))')
        require(driver.call('desktop', 'evaluate', expression='window.talos.workerIdentity().then(r=>r.data.nodeId)') == local,
                'Native Desktop restart changed its Computer identity')
        driver.call('desktop', 'evaluate', expression='location.hash="#/computers"')
        driver.call('desktop', 'wait', expression=f'Boolean(document.querySelector(\'main [data-computer-id="{local}"]\'))')
        require('This computer' in driver.call('desktop', 'evaluate', expression=f'document.querySelector(\'main [data-computer-id="{local}"]\').getAttribute("aria-label")'),
                'Native Desktop failed to identify its own embedded Worker')
        wait_for(lambda: any(node['nodeId'] == local and node['available'] for node in cluster.list_nodes('alice')),
                 'Native Desktop registration and Connect restored')
        driver.call('desktop', 'wait', expression='!document.querySelector("main .text-danger")')
        driver.call('desktop', 'viewport', width=1440, height=960)
        driver.call('desktop', 'screenshot', path=str(REPORT / 'desktop-local-computer.png'))
        driver.call('desktop', 'evaluate', expression='location.hash="#/settings/models"')
        driver.call('desktop', 'wait', expression='Boolean(document.querySelector("main input")) && document.querySelector("main").innerText.includes("DeepSeek")')
        driver.click('desktop', 'DeepSeek')
        driver.call('desktop', 'screenshot', path=str(REPORT / 'desktop-owner-models.png'))
        print('CASE sync_real_owner_ui_settings_shared_history_and_native_local_computer_screenshots PASS', flush=True)
    finally:
        driver.close()
