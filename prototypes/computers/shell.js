/* Keep the entered Computer independent from the node inspected in management. */
const shellState = { page: 'management', computerId: 'macbook-pro', sidebarCollapsed: false, pickerQuery: '', projects: new Map(), drafts: new Map() };
const resourceMedia = window.matchMedia('(min-width:1024px)');
const resourceSidebar = $('#resource-sidebar');
const workspaceFixtures = {
  'macbook-pro': { projects: ['talos', 'design-system'], conversations: ['完善 Computer 连接视图', '梳理同步数据流', '调整桌面布局'] },
  'mac-studio': { projects: ['studio', 'desktop-client'], conversations: ['检查桌面构建', '整理工作区'] },
  'gpu-worker': { projects: ['model-evaluation', 'inference-lab'], conversations: ['运行模型评估', '检查 GPU 环境'] },
  'build-worker': { projects: ['release-builds', 'ci-tools'], conversations: ['检查构建产物', '查看打包日志'] },
  'lab-worker': { projects: ['experiments', 'benchmarks'], conversations: ['比较运行结果', '查看实验记录'] },
  'macbook-air': { projects: ['notes', 'personal-tools'], conversations: ['整理本地笔记', '继续之前的任务'] }
};
function activeComputer() { return devices.find((device) => device.id === shellState.computerId); }
function closeRailPicker(restoreFocus = false) {
  $('#rail-picker').hidden = true; $('#rail-overflow').setAttribute('aria-expanded', 'false');
  if (restoreFocus) $('#rail-overflow').focus();
}
function renderRailPicker() {
  const matches = devices.filter((device) => `${device.name} ${device.system}`.toLowerCase().includes(shellState.pickerQuery.toLowerCase()));
  $('#rail-picker-items').innerHTML = matches.length ? matches.map((device) => `<button class="picker-row ${device.id === shellState.computerId ? 'active' : ''}" data-enter="${device.id}" aria-label="进入 ${escapeHTML(device.name)}">${icon(device.local ? 'laptop' : 'monitor')}<span>${escapeHTML(device.name)}${device.local ? ' · 本机' : ''}<small>${device.system} · ${device.online ? '已连接' : '离线'}</small></span><i class="dot ${device.online ? '' : 'offline-dot'}"></i></button>`).join('') : '<p class="picker-empty">没有找到 Computer</p>';
  $('#rail-picker-items').querySelectorAll('[data-enter]').forEach((button) => { button.onclick = () => openComputerWorkspace(button.dataset.enter); });
}
function renderComputerRail() {
  const quick = [...new Set(['macbook-pro', shellState.computerId, ...devices.map((device) => device.id)])].slice(0, 3);
  $('#rail-devices').innerHTML = quick.map((id) => {
    const device = devices.find((item) => item.id === id), selected = id === shellState.computerId;
    return `<button class="rail-button rail-device ${selected ? 'active' : ''}" data-enter="${id}" title="${escapeHTML(device.name)}${device.local ? ' · 本机' : ''}" aria-label="进入 ${escapeHTML(device.name)}${device.local ? '（本机）' : ''}" aria-pressed="${selected}">${icon(device.local ? 'laptop' : 'monitor')}<span class="dot ${device.online ? '' : 'offline-dot'}"></span></button>`;
  }).join('');
  $('#rail-devices').querySelectorAll('[data-enter]').forEach((button) => { button.onclick = () => openComputerWorkspace(button.dataset.enter); });
  if (shellState.page === 'management') $('#rail-manage').setAttribute('aria-current', 'page'); else $('#rail-manage').removeAttribute('aria-current');
  renderRailPicker();
}
function updateResourceSidebar() {
  resourceSidebar.close();
  if (shellState.page === 'workspace' && resourceMedia.matches && !shellState.sidebarCollapsed) resourceSidebar.open = true;
  $('#expand-resources').hidden = resourceMedia.matches && resourceSidebar.open;
}
function showManagement() {
  shellState.page = 'management'; closeRailPicker();
  $('#workspace-page').hidden = true; $('#management-page').hidden = false;
  updateResourceSidebar(); renderComputerRail(); requestAnimationFrame(drawWires);
  $('#rail-manage').focus();
}
function openComputerWorkspace(id) {
  shellState.computerId = id; shellState.page = 'workspace';
  shellState.sidebarCollapsed = false; closeRailPicker();
  $('#management-page').hidden = true; $('#workspace-page').hidden = false;
  renderWorkspace(); updateResourceSidebar(); renderComputerRail();
  $('#workspace-draft').focus();
}
function renderWorkspace() {
  const device = activeComputer(), fixture = workspaceFixtures[device.id];
  const selectedProject = shellState.projects.get(device.id) || fixture.projects[0];
  $('#workspace-computer-name').textContent = device.name;
  $('#workspace-header-icon').innerHTML = icon(device.local ? 'laptop' : 'monitor');
  $('#workspace-online-dot').classList.toggle('offline-dot', !device.online);
  $('#workspace-availability').textContent = device.online ? '已连接' : '离线';
  $('#resource-computer-label').innerHTML = `<span class="resource-computer-name">${device.online ? '已连接到' : '离线'} ${escapeHTML(device.name)}</span><span class="dot ${device.online ? '' : 'offline-dot'}"></span>`;
  $('#workspace-offline').hidden = device.online;
  $('#workspace-context').innerHTML = `<span class="dot ${device.online ? '' : 'offline-dot'}"></span>${escapeHTML(device.name)}${device.local ? ' · 本机' : ''} <span> / </span> ${device.type}`;
  $('#workspace-projects').innerHTML = fixture.projects.map((project, i) => `<button class="resource-project ${project === selectedProject ? 'active' : ''}" data-project="${project}">${icon('folder')}<span>${project}</span><span class="resource-project-count">${i === 0 ? device.sessions : 2}</span></button>`).join('');
  $('#workspace-conversations').innerHTML = fixture.conversations.map((label, i) => `<button class="resource-conversation" data-conversation="${i}"><span>${label}</span><small>${device.online ? '今天' : '昨天'}</small></button>`).join('');
  $('#workspace-project-name').textContent = selectedProject;
  $('#workspace-draft').value = shellState.drafts.get(device.id) || '';
  $('#workspace-draft').disabled = !device.online;
  $('#workspace-send').disabled = !device.online;
  $('#workspace-caption').textContent = device.online ? `在 ${device.name} 上开始任务 · ${selectedProject} · 仅为布局预览` : '当前 Computer 离线，可浏览历史；不会自动切换执行设备。';
  $('#workspace-projects').querySelectorAll('[data-project]').forEach((button) => { button.onclick = () => { shellState.projects.set(device.id, button.dataset.project); renderWorkspace(); }; });
  $('#workspace-conversations').querySelectorAll('[data-conversation]').forEach((button) => { button.onclick = () => toast(`${device.name} · ${fixture.conversations[Number(button.dataset.conversation)]} · 历史入口示意`); });
}
$('#rail-home').onclick = () => openComputerWorkspace(shellState.computerId);
$('#rail-manage').onclick = showManagement;
$('#picker-manage').onclick = showManagement;
$('#workspace-manage').onclick = showManagement;
$('#rail-overflow').onclick = () => {
  const open = $('#rail-picker').hidden;
  $('#rail-picker').hidden = !open; $('#rail-overflow').setAttribute('aria-expanded', open);
  if (open) { shellState.pickerQuery = ''; $('#rail-search').value = ''; renderRailPicker(); $('#rail-search').focus(); }
};
$('#rail-search').oninput = (event) => { shellState.pickerQuery = event.target.value.trim(); renderRailPicker(); };
$('#rail-picker').onkeydown = (event) => {
  if (event.key === 'Escape') { event.stopPropagation(); closeRailPicker(true); }
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    const controls = [...$('#rail-picker').querySelectorAll('input,button')];
    const index = controls.indexOf(document.activeElement), next = (index + (event.key === 'ArrowDown' ? 1 : controls.length - 1)) % controls.length;
    controls[next]?.focus(); event.preventDefault();
  }
};
document.addEventListener('pointerdown', (event) => { if (!event.target.closest('.rail-overflow-anchor')) closeRailPicker(); });
$('#collapse-resources').onclick = () => { shellState.sidebarCollapsed = true; resourceSidebar.close(); $('#expand-resources').hidden = false; $('#expand-resources').focus(); };
$('#expand-resources').onclick = () => { shellState.sidebarCollapsed = false; if (resourceMedia.matches) updateResourceSidebar(); else resourceSidebar.showModal(); };
resourceSidebar.oncancel = () => { shellState.sidebarCollapsed = true; };
resourceSidebar.onclick = (event) => {
  if (event.target !== resourceSidebar || resourceMedia.matches) return;
  const rect = resourceSidebar.getBoundingClientRect();
  if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) resourceSidebar.close();
};
resourceMedia.addEventListener('change', updateResourceSidebar);
$('#workspace-home').onclick = () => { $('#workspace-heading').textContent = '从一个清晰的问题开始'; };
$('#new-conversation').onclick = () => { shellState.drafts.set(shellState.computerId, ''); $('#workspace-draft').value = ''; if (!resourceMedia.matches) resourceSidebar.close(); $('#workspace-draft').focus(); };
$('#workspace-draft').oninput = (event) => shellState.drafts.set(shellState.computerId, event.target.value);
$('#workspace-send').onclick = () => toast('布局预览：演示任务不会发送给 Computer');
$('#workspace-project').onclick = () => { const device = activeComputer(), projects = workspaceFixtures[device.id].projects; const current = shellState.projects.get(device.id) || projects[0]; shellState.projects.set(device.id, projects[(projects.indexOf(current) + 1) % projects.length]); renderWorkspace(); };
$('#workspace-add-project').onclick = () => toast('项目创建入口示意 · 本轮不创建真实项目');
$('#rail-account').onclick = () => dialog('个人账号', '<p>账号入口保留在最左侧 Computer 栏底部，与现有页面布局一致。</p><p>当前仅为演示账号，不进行登录或账号变更。</p>');
document.querySelectorAll('[data-workspace-nav]').forEach((button) => { button.onclick = () => toast(`${activeComputer().name} · ${button.dataset.workspaceNav}入口示意`); });
renderComputerRail();
