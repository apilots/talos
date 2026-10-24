"""Compiled production-control visual fixtures; no destructive or model API requests."""
from fixture import REPORT, require
from playwright.sync_api import expect


def settle_control(control):
    control.evaluate('''async node => {
        node.getBoundingClientRect();
        await Promise.all(node.getAnimations().map(animation => animation.finished.catch(() => {})));
    }''')


def destructive_action_theme(page, theme):
    """Render the two production dialog callers with real compiled CSS, without deleting data."""
    observations = []
    for kind, title, label in (('project', 'Remove Theme review?', 'Remove'), ('session', 'Delete “Theme review”?', 'Delete')):
        def render(pending):
            page.evaluate('''async ({kind, pending}) => {
                const {default: React} = await import('/node_modules/.vite/deps/react.js');
                const {default: ReactDOM} = await import('/node_modules/.vite/deps/react-dom_client.js');
                if (!window.themeDialogFixture) {
                    const root = document.createElement('div');
                    document.body.append(root);
                    window.themeDialogFixture = {root:ReactDOM.createRoot(root)};
                }
                const fixture = window.themeDialogFixture;
                const onClose = () => fixture.root.render(null);
                const onDestructive = () => { throw new Error('Visual fixture must not submit destructive actions'); };
                const Component = kind === 'project'
                    ? (await import('/src/widgets/sidebar/SidebarProjectRemoveDialog.tsx')).SidebarProjectRemoveDialog
                    : (await import('/src/widgets/sidebar/SidebarSessionDeleteDialog.tsx')).SidebarSessionDeleteDialog;
                fixture.root.render(React.createElement(Component, kind === 'project'
                    ? {projectName:'Theme review', removing:pending, onClose, onRemove:onDestructive}
                    : {sessionTitle:'Theme review', deleting:pending, onClose, onDelete:onDestructive}));
            }''', {'kind': kind, 'pending': pending})
        render(False)
        heading = page.get_by_role('heading', name=title, exact=True)
        expect(heading).to_be_visible()
        dialog = page.locator('div.relative.z-10').filter(has=heading)
        button = dialog.get_by_role('button', name=label, exact=True)
        def colors():
            settle_control(button)
            return button.evaluate('''node => {
                const style = getComputedStyle(node);
                const danger = getComputedStyle(document.documentElement).getPropertyValue('--color-danger').trim().split(' ').filter(Boolean).join(', ');
                return {fill:style.backgroundColor, text:style.color, opacity:style.opacity, danger};
            }''')
        initial = colors()
        require(initial['fill'] == 'rgba(' + initial['danger'] + ', 0.15)', 'Destructive action did not use the shared danger fill')
        button.hover()
        hovered = colors()
        require(hovered['fill'] == 'rgba(' + hovered['danger'] + ', 0.25)', 'Primary hover specificity overrode the destructive role')
        require(hovered['text'] == 'rgb(' + hovered['danger'] + ')', 'Destructive hover changed its danger text')
        page.screenshot(path=str(REPORT / ('danger-' + kind + '-' + theme + '.png')), full_page=True)
        render(True)
        button = dialog.locator('button').last
        expect(button).to_be_disabled()
        pending = colors()
        require(pending['fill'] == initial['fill'] and pending['opacity'] == '0.5', 'Disabled destructive action retained enabled hover styling')
        dialog.get_by_role('button', name='Cancel', exact=True).click()
        expect(heading).to_have_count(0)
        observations.append({'kind': kind, 'initial': initial, 'hovered': hovered, 'pending': pending})
    page.evaluate('window.themeDialogFixture.root.unmount(); delete window.themeDialogFixture;')
    return observations


def model_accent_theme(page, theme):
    """Preserve main's feature-blue reasoning controls using the production picker."""
    page.evaluate('''async () => {
        const {default: React} = await import('/node_modules/.vite/deps/react.js');
        const {default: ReactDOM} = await import('/node_modules/.vite/deps/react-dom_client.js');
        const {ComposerModelPicker} = await import('/src/features/chat/components/ComposerModelPicker.tsx');
        const host = document.createElement('div');
        Object.assign(host.style, {position:'fixed', top:'12px', left:'650px', zIndex:'2000'});
        document.body.append(host);
        const root = ReactDOM.createRoot(host);
        window.themeModelFixture = {root, host};
        function Fixture() {
            const [level, setLevel] = React.useState('medium');
            return React.createElement(ComposerModelPicker, {
                models:[{id:'theme:model', provider:'theme', name:'Theme model', reasoningEffort:'medium',
                    selectionCapabilities:{reasoning:'effort', fast:true, default_fast:false, default_reasoning_effort:'medium'}}],
                selectedModel:'theme:model', reasoningLevel:level, onReasoningLevelChange:setLevel,
            });
        }
        root.render(React.createElement(Fixture));
    }''')
    try:
        trigger = page.get_by_role('button', name='Theme model · Medium', exact=True)
        trigger.click()
        dialog = page.get_by_role('dialog', name='Model and reasoning settings', exact=True)
        level = dialog.get_by_text('Medium', exact=True)
        fill = dialog.locator('[class*="effortFill"]')
        require(level.evaluate('node => getComputedStyle(node).color') == 'rgb(59, 130, 246)', 'Reasoning label lost its pure feature-blue accent')
        require(fill.evaluate('node => getComputedStyle(node).backgroundColor') == 'rgb(59, 130, 246)', 'Reasoning slider used a muted status color')
        slider = dialog.get_by_role('slider', name='Reasoning depth', exact=True)
        slider.focus()
        slider.press('ArrowRight')
        expect(dialog.get_by_text('High', exact=True)).to_be_visible()
        fast = dialog.locator('button[aria-pressed]')
        fast.click()
        expect(fast).to_have_attribute('aria-pressed', 'true')
        require(fast.evaluate('node => getComputedStyle(node).color') == 'rgb(59, 130, 246)', 'Fast state lost its feature-blue accent')
        page.screenshot(path=str(REPORT / ('model-accent-' + theme + '.png')), full_page=True)
        page.keyboard.press('Escape')
        expect(dialog).not_to_be_visible()
        return {'reasoning':'rgb(59, 130, 246)', 'slider':'rgb(59, 130, 246)', 'fast':'rgb(59, 130, 246)', 'keyboardLevel':'High'}
    finally:
        page.evaluate('window.themeModelFixture.root.unmount(); window.themeModelFixture.host.remove(); delete window.themeModelFixture;')
