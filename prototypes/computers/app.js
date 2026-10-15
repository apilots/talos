const $ = (selector) => document.querySelector(selector);
const icons = {
  home: '<path d="m3 10 9-7 9 7v10a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1z"/>', more: '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>', manage: '<rect x="3" y="3" width="18" height="13" rx="2"/><path d="M8 21h8m-4-5v5"/><circle cx="12" cy="9.5" r="3"/><path d="M12 5v1m0 7v1m-4-4h1m6 0h1"/>', panel: '<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M9 3v18"/>', 'arrow-up': '<path d="M12 19V5m-6 6 6-6 6 6"/>',
  laptop: '<rect x="4" y="4" width="16" height="12" rx="2"/><path d="M2 20h20l-2-4H4z"/>', monitor: '<rect x="3" y="3" width="18" height="13" rx="2"/><path d="M8 21h8m-4-5v5"/>',
  server: '<rect x="4" y="3" width="16" height="7" rx="2"/><rect x="4" y="14" width="16" height="7" rx="2"/><path d="M8 6h.1M8 17h.1m7-11h2m-2 11h2"/>',
  sync: '<path d="m17 3 4 4-4 4M21 7H8a5 5 0 0 0-5 5m4 9-4-4 4-4m-4 4h13a5 5 0 0 0 5-5"/>',
  search: '<circle cx="10" cy="10" r="6"/><path d="m15 15 5 5"/>', plus: '<path d="M12 5v14M5 12h14"/>',
  compose: '<path d="m15 4 5 5M4 20l5-1L21 7l-4-4L5 15z"/>', brain: '<path d="M12 5c-4-6-10 0-7 4-5 3-2 9 2 9 1 5 5 3 5 0V5c4-6 10 0 7 4 5 3 2 9-2 9-1 5-5 3-5 0M6 9l2 2m10-2-2 2"/>',
  blocks: '<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/>',
  folder: '<path d="M3 7V5a1 1 0 0 1 1-1h6l2 3h8a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z"/>',
  topology: '<rect x="2" y="8" width="6" height="8" rx="1"/><rect x="16" y="2" width="6" height="6" rx="1"/><rect x="16" y="16" width="6" height="6" rx="1"/><path d="M8 12h4V5h4m-4 7v7h4"/>',
  list: '<path d="M9 5h12M9 12h12M9 19h12M3 5h1M3 12h1M3 19h1"/>', layers: '<path d="m12 3 10 5-10 5L2 8zm-10 10 10 5 10-5M2 18l10 5 10-5"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 6v6l4 2"/>', info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6m0-10v1"/>', chevron: '<path d="m9 6 6 6-6 6"/>'
};
function icon(name) { return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${icons[name] || icons.monitor}</svg>`; }
function hydrateIcons(root = document) { root.querySelectorAll('[data-icon]').forEach((node) => { node.innerHTML = icon(node.dataset.icon); }); }
const devices = [
  { id: 'macbook-pro', name: 'MacBook Pro', type: 'Desktop', system: 'macOS 15.6', arch: 'arm64', cpu: 'Apple M4 Pro', memory: '48 GiB', online: true, manager: true, local: true, sync: 'synced', pending: 0, latency: '32 ms', activity: '刚刚', heartbeat: '2 秒前', transport: 'Connect', version: '0.1.9', sessions: 12 },
  { id: 'mac-studio', name: 'Mac Studio', type: 'Desktop', system: 'macOS 15.6', arch: 'arm64', cpu: 'Apple M3 Ultra', memory: '96 GiB', online: true, manager: true, sync: 'pending', pending: 12, latency: '840 ms', activity: '刚刚', heartbeat: '3 秒前', transport: 'Connect', version: '0.1.9', sessions: 8 },
  { id: 'gpu-worker', name: 'gpu-worker-01', type: 'Worker', system: 'Ubuntu 24.04', arch: 'x86_64', cpu: 'AMD EPYC 7443P', memory: '128 GiB', online: true, manager: false, sync: 'none', activity: '8 秒前', heartbeat: '8 秒前', transport: 'Connect', version: '0.1.9', sessions: 4 },
  { id: 'build-worker', name: 'build-worker', type: 'Worker', system: 'Debian 12', arch: 'x86_64', cpu: 'Intel Xeon', memory: '32 GiB', online: true, manager: false, sync: 'none', activity: '12 秒前', heartbeat: '12 秒前', transport: 'Direct', version: '0.1.9', sessions: 2 },
  { id: 'lab-worker', name: 'lab-worker', type: 'Worker', system: 'Ubuntu 24.04', arch: 'arm64', cpu: 'Ampere Altra', memory: '64 GiB', online: true, manager: false, sync: 'none', activity: '16 秒前', heartbeat: '16 秒前', transport: 'Connect', version: '0.1.9', sessions: 0 },
  { id: 'macbook-air', name: 'MacBook Air', type: 'Desktop', system: 'macOS 15.5', arch: 'arm64', cpu: 'Apple M2', memory: '16 GiB', online: false, manager: true, sync: 'offline', activity: '18 小时前', heartbeat: '18 小时前', transport: 'Connect', version: '0.1.8', sessions: 5 }
];
const state = { selected: 'macbook-pro', view: 'topology', layer: 'all', query: '', status: 'all', localSync: true };
const escapeHTML = (value) => String(value).replace(/[&<>"']/g, (char) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[char]));
function toast(message) { $('#toast').textContent = message; $('#toast').hidden = false; clearTimeout(toast.timer); toast.timer = setTimeout(() => { $('#toast').hidden = true; }, 3000); }
function dialog(title, body) { $('#dialog-title').textContent = title; $('#dialog-body').innerHTML = body; $('#dialog').showModal(); }
function filteredDevices() { return devices.filter((device) => `${device.name} ${device.type} ${device.system}`.toLowerCase().includes(state.query.toLowerCase()) && (state.status === 'all' || state.status === 'online' && device.online || state.status === 'offline' && !device.online || state.status === 'pending' && device.sync === 'pending')); }
function syncLabel(device) { return !device.manager ? '不参与账号同步' : device.local && !state.localSync ? '本机同步已关闭' : device.sync === 'pending' ? `${device.pending} 项待同步` : device.sync === 'offline' ? '同步离线' : '账号数据已同步'; }
function selectNode(id) { state.selected = id; $('.content-grid').classList.add('has-selected'); render(); }
function renderDevices() {
  const visible = filteredDevices();
  $('#visible-count').textContent = `${visible.length} / 6 台设备`;
  $('#device-nodes').innerHTML = visible.map((device, i) => `<button class="device-node ${state.selected === device.id ? 'selected' : ''} ${!device.online ? 'offline' : ''} ${device.disabled ? 'disabled' : ''} ${state.layer === 'sync' && !device.manager ? 'layer-dim' : ''}" style="top:${63 + i * 101}px;animation-delay:${i * 25}ms" data-device="${device.id}" aria-pressed="${state.selected === device.id}" aria-label="${escapeHTML(device.name)}，${device.online ? '在线' : '离线'}，${syncLabel(device)}"><span class="device-top"><span class="device-icon">${icon(device.manager ? 'laptop' : 'server')}</span><span class="device-name">${escapeHTML(device.name)}${device.local ? '<span class="local-tag">本机</span>' : ''}</span><span class="device-status"><i class="dot ${device.online ? '' : 'offline-dot'}"></i>${device.disabled ? '远程停用' : device.online ? '在线' : '离线'}</span></span><span class="device-meta">${device.type} <span>·</span> ${device.system} <span>·</span> ${device.arch}</span><span class="device-bottom"><span class="sync-status ${device.sync === 'pending' ? 'pending' : ''} ${!device.manager ? 'no-sync' : ''}">${device.manager ? icon('sync') : ''}${syncLabel(device)}</span><span>${device.online ? device.transport : '18h 前在线'}</span></span></button>`).join('');
  $('#canvas-empty').hidden = visible.length > 0;
  $('#list-empty').hidden = visible.length > 0;
  $('#list-body').innerHTML = visible.map((device) => `<tr data-device="${device.id}" class="${state.selected === device.id ? 'selected' : ''}"><td><button class="list-name" aria-label="查看 ${escapeHTML(device.name)} 详情">${icon(device.manager ? 'laptop' : 'server')}<span>${escapeHTML(device.name)}${device.local ? '<span class="local-tag">本机</span>' : ''}<small>${device.id}</small></span></button></td><td>${device.type}<span class="list-sub">${device.system}</span></td><td><span class="list-state"><span class="dot ${device.online ? '' : 'offline-dot'}"></span>${device.disabled ? '远程停用' : device.online ? device.transport : '离线'}</span></td><td class="${device.sync === 'pending' ? 'amber' : device.manager ? 'purple' : ''}">${syncLabel(device)}</td><td>${device.activity}</td><td>↗</td></tr>`).join('');
  document.querySelectorAll('[data-device]').forEach((node) => { node.onclick = () => selectNode(node.dataset.device); });
}
function row(label, value, code = false) { return `<div class="detail-row"><dt>${label}</dt><dd class="${code ? 'code' : ''}">${escapeHTML(value)}</dd></div>`; }
function renderDetails() {
  const device = devices.find((item) => item.id === state.selected);
  if (!device) { renderServiceDetails(); return; }
  $('#detail-panel').innerHTML = `<div class="detail-header"><div class="detail-eyebrow">COMPUTER DETAILS<span>↗</span></div><div class="detail-hero"><span class="detail-hero-icon">${icon(device.manager ? 'laptop' : 'server')}</span><div><h2>${escapeHTML(device.name)}</h2><p>${device.local ? '当前设备 · ' : ''}${device.type}${device.manager ? ' + 内嵌 Worker' : ' · 独立执行节点'}</p></div></div><div class="detail-status"><span class="dot ${device.online ? '' : 'offline-dot'}"></span>${device.disabled ? '远程入口已停用' : device.online ? '已连接 Control Plane' : '离线，保留历史数据'}</div></div><div class="detail-content"><div><h3 class="detail-section-title">设备信息</h3><dl>${row('系统', device.system)}${row('处理器', device.cpu)}${row('内存 / 架构', `${device.memory} / ${device.arch}`)}${row('Worker 版本', device.version)}${row('节点 ID', device.id, true)}</dl><div class="detail-divider"></div><h3 class="detail-section-title">执行链路</h3><dl>${row('控制面', 'agent.talos.love', true)}${row('连接方式', device.online ? device.transport : '当前不可用')}${row('最近心跳', device.heartbeat)}</dl></div><div><div class="sync-detail ${device.sync === 'pending' ? 'pending-detail' : ''}"><div><strong>${device.manager ? '账号同步' : '独立 Worker'}</strong><span>${device.manager ? 'WSS' : 'RPC'}</span></div><p>${device.manager ? `${syncLabel(device)}<br>${device.online && (state.localSync || !device.local) ? `上传 ↑ ${device.local ? '4.2' : '2.1'} KB/s　下载 ↓ ${device.local ? '8.6' : '6.4'} KB/s<br>持久确认耗时 ${device.latency} · 演示数据` : device.local && !state.localSync ? '已停止传输 · 上次追平：刚刚' : '最后同步：18 小时前'}<br>经 Talos Sync 共享已提交的会话与目录事实。` : '纯 Worker 不连接 Talos Sync。<br>执行结果返回当前协调管理端，由管理端落库后同步。'}</p></div><div class="detail-actions"><button class="button" id="rename">重命名</button><button class="button" id="disable">${device.disabled ? '启用远程' : '停用远程'}</button></div><button class="button primary open-computer" id="open-computer">${device.online ? '进入 Computer' : '查看历史记录'} <span>↗</span></button><p class="detail-hint">${device.online ? '选择设备仅切换查看范围，已运行的任务继续留在原 Computer。' : '设备离线时无法执行新任务；保留历史，不自动切换到其他设备。'}<br>本页为设计预览，不会修改真实设备。</p></div></div>`;
  $('#rename').onclick = () => { dialog('重命名 Computer', `<p>仅修改当前演示设备名称。</p><label for="new-name">设备名称</label><input id="new-name" maxlength="255" value="${escapeHTML(device.name)}"><button class="button primary" id="save-name">保存演示名称</button>`); $('#save-name').onclick = () => { const name = $('#new-name').value.trim(); if (!name) { $('#new-name').setCustomValidity('请输入设备名称'); $('#new-name').reportValidity(); return; } device.name = name; $('#dialog').close(); render(); toast('演示名称已更新'); }; };
  $('#disable').onclick = () => { device.disabled = !device.disabled; render(); toast(`已${device.disabled ? '停用' : '启用'}演示设备的远程入口，在线状态与同步保持独立`); };
  $('#open-computer').onclick = () => openComputerWorkspace(device.id);
}
function renderServiceDetails() {
  const sync = state.selected === 'sync';
  $('#detail-panel').innerHTML = `<div class="detail-header"><div class="detail-eyebrow">INFRASTRUCTURE<span>固定节点</span></div><div class="detail-hero"><span class="detail-hero-icon">${icon(sync ? 'sync' : 'layers')}</span><div><h2>${sync ? 'Talos Sync' : 'Control Plane'}</h2><p>${sync ? 'sync.talos.love' : 'agent.talos.love'}</p></div></div><div class="detail-status"><span class="dot"></span>演示服务在线</div></div><div class="detail-content"><div><h3 class="detail-section-title">${sync ? '共享已提交事实' : '协调执行与连接'}</h3><p class="service-description">${sync ? '同一账号的管理端通过 WSS 双向同步。服务合并并持久化已提交事实，再分发给其他管理端。纯 Worker 不连接此服务。' : 'Worker 主动注册和续租，执行通过 Direct 或 Connect 路由。资源和执行结果回到当前协调管理端，不通过 Sync 中转。'}</p><dl>${row('连接协议', sync ? 'WSS / Loro' : 'Direct / Connect')}${row('当前范围', '当前演示账号')}${row(sync ? '同步管理端' : '在线 Computer', sync ? (state.localSync ? '3 个（含控制面）' : '2 个（含控制面）') : '5 / 6 台')}</dl></div><div><div class="path-card">${sync ? 'Desktop ↔ Talos Sync<br>Control Plane ↔ Talos Sync<br>不传输执行 RPC 或 Workspace 文件' : '管理端 → Control Plane → Worker<br>Worker → 当前协调管理端<br>结果落库 → 异步账号同步'}</div><p class="detail-hint">${sync ? '全局 Sync Metrics 只提供匿名汇总。此账号的设备关系需由 Talos 授权接口组合，不能由公开 Metrics 推导。' : '固定节点表示当前连接的服务，不计入 Computer 数量；作为管理宿主时可启用账号同步。'}</p><button class="button open-computer" id="service-more">${sync ? '同步与执行的区别' : '查看链路说明'}</button></div></div>`;
  $('#service-more').onclick = showFlowHelp;
}
function drawWires() {
  if (state.view !== 'topology') return;
  const world = $('#world').getBoundingClientRect();
  if (!world.width) return;
  const bounds = (node) => { const rect = node.getBoundingClientRect(); return { x: rect.left - world.left, y: rect.top - world.top, w: rect.width, h: rect.height }; };
  const cp = bounds($('#control-plane')), sync = bounds($('#sync-service'));
  const visible = filteredDevices();
  let wires = '<defs><marker id="exec-arrow" markerWidth="5" markerHeight="5" refX="4" refY="2.5" orient="auto-start-reverse"><path d="M0 0L5 2.5L0 5" fill="none" stroke="#b6b7c1" stroke-width=".8"/></marker><marker id="sync-arrow" markerWidth="5" markerHeight="5" refX="4" refY="2.5" orient="auto-start-reverse"><path d="M0 0L5 2.5L0 5" fill="none" stroke="#a18dca" stroke-width=".8"/></marker></defs>';
  const relation = `M${cp.x + 48} ${cp.y + cp.h} C${cp.x + 48} ${cp.y + cp.h + 55} ${sync.x + 48} ${sync.y - 55} ${sync.x + 48} ${sync.y}`;
  if (state.layer !== 'execution') wires += `<path class="wire sync-wire" d="${relation}" marker-start="url(#sync-arrow)" marker-end="url(#sync-arrow)"><title>Control Plane 管理宿主与 Sync 双向共享账号事实</title></path>`;
  visible.forEach((device) => {
    const node = document.querySelector(`.device-node[data-device="${device.id}"]`);
    const box = bounds(node), focused = state.selected === device.id, dimmed = devices.some((item) => item.id === state.selected) && !focused;
    const sourceX = cp.x + cp.w, targetX = box.x - 6, executionY = box.y + (device.manager ? 29 : 42);
    const bend = sourceX + (targetX - sourceX) * .5;
    const path = `M${sourceX} ${cp.y + 42} C${bend} ${cp.y + 42} ${bend} ${executionY} ${targetX} ${executionY}`;
    if (state.layer !== 'sync') wires += `<path class="wire ${!device.online ? 'offline-wire' : ''} ${focused ? 'focused' : ''} ${dimmed ? 'muted-wire' : ''}" d="${path}" marker-start="url(#exec-arrow)" marker-end="url(#exec-arrow)"><title>${escapeHTML(device.name)}：${device.online ? '执行与结果双向流动' : '离线，链路不可用'}</title></path>`;
    if (device.manager && state.layer !== 'execution') {
      const y = box.y + 64, sX = sync.x + sync.w, bendX = sX + (targetX - sX) * .56;
      const syncPath = `M${sX} ${sync.y + 43} C${bendX} ${sync.y + 43} ${bendX} ${y} ${targetX} ${y}`;
      const active = device.online && (!device.local || state.localSync);
      wires += `<path class="wire sync-wire ${!active ? 'offline-wire' : ''} ${focused ? 'focused' : ''} ${dimmed ? 'muted-wire' : ''}" d="${syncPath}" ${active ? 'marker-start="url(#sync-arrow)" marker-end="url(#sync-arrow)"' : ''}><title>${escapeHTML(device.name)}：${active ? '双向同步已提交事实' : '同步链路不可用'}</title></path>`;
      if (active && focused) wires += `<circle class="flow-dot" r="2.5" style="offset-path:path('${syncPath}');animation-delay:-1s"/><circle class="flow-dot" r="2" style="offset-path:path('${syncPath}');animation-direction:reverse;animation-delay:-2s"/>`;
    }
  });
  $('#wires').innerHTML = wires;
  $('.service-relation-label').style.opacity = state.layer === 'execution' ? .25 : 1;
  $('.exec-label').hidden = state.layer === 'sync'; $('.sync-label').hidden = state.layer === 'execution';
}
function render() { if (typeof renderComputerRail === 'function') renderComputerRail(); $('#manager-count').textContent = state.localSync ? '3' : '2'; renderDevices(); renderDetails(); requestAnimationFrame(drawWires); }
function setView(view) { state.view = view; $('#canvas-hint').hidden = view !== 'topology'; $('#canvas-viewport').hidden = view !== 'topology'; $('#list-container').hidden = view !== 'list'; $('#flow-filters').hidden = view !== 'topology'; $('#view-title').textContent = view === 'topology' ? '连接拓扑' : '全部 Computer'; ['topology', 'list'].forEach((name) => { $(`#${name}-view`).classList.toggle('active', view === name); $(`#${name}-view`).setAttribute('aria-pressed', view === name); }); render(); }
function showFlowHelp() { dialog('两种链路，各有职责', '<h3>灰色实线 ↔ 执行与资源回调</h3><p>Worker 向 Control Plane 注册，通过 Direct / Connect 接收任务。结果与资源返回当前协调管理端。Desktop 卡片包含管理进程和内嵌 Worker，合并表示在同一台电脑上。</p><h3>紫色虚线 ↔ 账号事实同步</h3><p>Desktop 和已启用同步的 Control Plane 经 Talos Sync 双向共享会话、Computer、Project 和 Timeline 事实。箭头代表协议流向，移动点只做示意。</p><h3>连接独立于同步</h3><p>关闭同步不停止 Worker 执行。纯 Worker 没有管理数据库，不直接连接 Sync。离线连线变淡，不能据此判断同步已追平。</p><p>图中仅展示当前账号可见的设备；所有数据均为设计演示。</p>'); }
$('#topology-view').onclick = () => setView('topology'); $('#list-view').onclick = () => setView('list');
$('#search').oninput = (event) => { state.query = event.target.value.trim(); render(); }; $('#status-filter').onchange = (event) => { state.status = event.target.value; render(); };
$('#clear-search').onclick = () => { state.query = ''; state.status = 'all'; $('#search').value = ''; $('#status-filter').value = 'all'; render(); };
$('#flow-filters').onclick = (event) => { const button = event.target.closest('button'); if (!button) return; state.layer = button.dataset.layer; $('#flow-filters').querySelectorAll('button').forEach((node) => { node.classList.toggle('active', node === button); node.setAttribute('aria-pressed', node === button); }); render(); };
$('#sync-toggle').onclick = () => { state.localSync = !state.localSync; $('#sync-toggle').setAttribute('aria-checked', state.localSync); $('#local-sync-description').textContent = state.localSync ? '已开启 · 当前演示账号 · 不影响其他设备的同步设置' : '已关闭 · 仅影响本机账号同步，执行链路继续可用'; render(); toast(`演示：本机账号同步已${state.localSync ? '开启' : '关闭'}`); };
$('#mode').onclick = () => dialog('切换连接模式', '<p>云端模式使用当前 Control Plane 注册和协调 Worker。本地模式由本机管理宿主提供服务。</p><p>真实模式切换需要现有 Desktop 连接流程重载状态，原型不模拟服务拓扑迁移。</p><button class="button" id="keep-mode">保持云端模式</button>');
$('#mode').addEventListener('click', () => { $('#keep-mode').onclick = () => $('#dialog').close(); });
$('#connect').onclick = () => dialog('连接一台 Computer', '<h3>Talos Desktop</h3><p>在另一台电脑上打开 Talos Desktop，并登录同一账号。内嵌 Worker 自动注册到 Control Plane。</p><h3>独立 Worker</h3><p>通过现有 Computer 创建接口领取一次性注册凭据，再按 Worker 部署说明连接当前控制面。</p><p>同步只在管理端之间发生；独立 Worker 无需接入 Sync。</p><p><code>Control Plane · agent.talos.love</code></p>');
$('#peer-info').onclick = () => dialog('管理端与 Computer 的口径', '<p>6 台 Computer 中有 5 台在线：2 台 Desktop 和 3 台纯 Worker。另有 1 台 Desktop 离线。</p><p>3 个在线同步管理端 = 2 台在线 Desktop + 当前 Control Plane 管理宿主。固定服务节点不计入 Computer 数量。</p><p>关闭本机同步后，在线同步管理端变为 2 个。真实实现需要查询各管理宿主的账号同步状态。</p>');
$('#flow-help').onclick = showFlowHelp; document.querySelectorAll('[data-service]').forEach((node) => { node.onclick = () => selectNode(node.dataset.service); });
document.querySelectorAll('[data-nav]').forEach((node) => { node.onclick = () => toast(`${node.dataset.nav} · 仅展示导航位置，本轮设计范围为 Computer 页面`); });
$('#close-dialog').onclick = () => $('#dialog').close();
document.addEventListener('keydown', (event) => { if (!$('#management-page').hidden && (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); $('#search').focus(); } });
new ResizeObserver(() => requestAnimationFrame(drawWires)).observe($('#world'));
hydrateIcons(); render();
