"""Private fixed-operation Browser Use adapter. Never expose this protocol as an MCP tool."""
import asyncio
import json
import sys

# Dependency logging must never share the result framing channel.
RESULT = sys.stdout
sys.stdout = sys.stderr

import websockets

# Match Browser Use's existing CDP connection budget and the Worker's cleanup
# budget. The outer Worker invocation/process deadline remains authoritative if
# a dependency does not cooperate with asyncio cancellation.
START_TIMEOUT_SECONDS = 15
STOP_TIMEOUT_SECONDS = 5


class StaleElement(Exception):
    pass


class Cdp:
    def __init__(self, ws):
        self.ws = ws
        self.sequence = 0

    async def call(self, method, params=None, session=None):
        self.sequence += 1
        request = {"id": self.sequence, "method": method, "params": params or {}}
        if session:
            request["sessionId"] = session
        await self.ws.send(json.dumps(request))
        for _ in range(4096):
            response = json.loads(await asyncio.wait_for(self.ws.recv(), 8))
            if response.get("id") != self.sequence:
                continue
            if "error" in response:
                raise RuntimeError("Browser protocol operation failed")
            return response.get("result", {})
        raise RuntimeError("Browser event capacity exceeded")


async def observe(request):
    async with websockets.connect(request["cdp"], origin=None, max_size=8 * 1024 * 1024, compression=None) as ws:
        cdp = Cdp(ws)
        targets = [target for target in (await cdp.call("Target.getTargets"))["targetInfos"]
                   if target["type"] == "page" and (target.get("url", "").startswith(("http://", "https://", "about:blank"))
                       or target.get("url") == "file:///opt/talos/browser/desktop/home.html")]
        target = next((target for target in targets if target["targetId"] == request.get("targetId")), None)
        if target is None:
            if request.get("targetId") and request["action"]["kind"] != "state":
                raise StaleElement()
            if not targets:
                raise RuntimeError("No website tab is available")
            target = targets[0]
        session = (await cdp.call("Target.attachToTarget", {"targetId": target["targetId"], "flatten": True}))["sessionId"]
        loader = (await cdp.call("Page.getFrameTree", session=session))["frameTree"]["frame"]["loaderId"]
        if request["action"]["kind"] == "screenshot":
            shot = await cdp.call("Page.captureScreenshot", {"format": "png", "captureBeyondViewport": False}, session)
            if len(shot["data"]) > 5 * 1024 * 1024:
                raise RuntimeError("Screenshot capacity exceeded")
            return {"page": {"targetId": target["targetId"], "url": target["url"], "title": target.get("title", "")},
                    "loaderId": loader, "elements": [], "screenshot": shot["data"]}
        snapshot = await cdp.call("DOMSnapshot.captureSnapshot", {"computedStyles": [], "includeDOMRects": True}, session)
        strings = snapshot["strings"]
        elements, text = [], []
        for document in snapshot["documents"][:16]:
            nodes = document["nodes"]
            # Node IDs are scoped to their target session. Frame-crossing actions must be re-observed separately.
            if strings[document["frameId"]] != (await cdp.call("Page.getFrameTree", session=session))["frameTree"]["frame"]["id"]:
                continue
            rendered=set(document.get("layout", {}).get("nodeIndex", []))
            for index, tag_index in enumerate(nodes["nodeName"][:10000]):
                if index not in rendered:
                    continue
                tag = strings[tag_index].lower()
                value = strings[nodes["nodeValue"][index]]
                if tag == "#text" and value.strip() and sum(map(len, text)) < 12000:
                    text.append(value.strip()[:500])
                attrs = nodes["attributes"][index]
                attrs = {strings[attrs[i]]: strings[attrs[i + 1]] for i in range(0, len(attrs), 2)}
                if len(elements) < 150 and (tag in ("input", "button", "select", "textarea", "a", "summary")
                                            or attrs.get("role") in ("button", "textbox", "link", "checkbox", "combobox")
                                            or "contenteditable" in attrs or "tabindex" in attrs):
                    safe_attrs = {key: value[:500] for key, value in attrs.items()
                                  if key in ("name", "type", "id", "placeholder", "aria-label", "role", "href", "title", "disabled")}
                    elements.append({"backendNodeId": nodes["backendNodeId"][index], "tag": tag,
                                     "attributes": safe_attrs, "targetId": target["targetId"], "loaderId": loader})
        return {"page": {"targetId": target["targetId"], "url": target["url"], "title": target.get("title", ""),
                         "tabs": [{"tabId": value["targetId"], "title": value.get("title", ""), "url": value["url"]} for value in targets[:32]],
                         "text": "\n".join(text)[:12000]}, "loaderId": loader, "elements": elements}


async def act(request):
    from browser_use import BrowserSession
    from browser_use.browser.events import SwitchTabEvent
    session = BrowserSession(cdp_url=request["cdp"], keep_alive=True, enable_default_extensions=False,
                             auto_download_pdfs=False, highlight_elements=False, downloads_path=request["downloads"])
    try:
        await asyncio.wait_for(session.start(), START_TIMEOUT_SECONDS)
        action = request["action"]
        kind = action["kind"]
        target_id = request.get("targetId")
        if target_id and kind != "tab":
            pages = await session.get_pages()
            if not any(value._target_id == target_id for value in pages):
                raise StaleElement()
            event = session.event_bus.dispatch(SwitchTabEvent(target_id=target_id))
            await event
            await event.event_result(raise_if_any=True, raise_if_none=False)
        page = None if kind == "tab" else await session.must_get_current_page()
        if target_id and kind != "tab" and (await page.get_target_info())["targetId"] != target_id:
            raise StaleElement()
        if kind == "navigate":
            await page.goto(action["url"])
        elif kind in ("click", "fill", "upload"):
            bound = request["element"]
            info = await page.get_target_info()
            frame = await session.cdp_client.send.Page.getFrameTree(session_id=await page.session_id)
            if info["targetId"] != bound["targetId"] or frame["frameTree"]["frame"]["loaderId"] != bound["loaderId"]:
                raise StaleElement()
            description = await session.cdp_client.send.DOM.describeNode(
                {"backendNodeId": bound["backendNodeId"]}, session_id=await page.session_id)
            raw_attrs = description["node"].get("attributes", [])
            attrs = dict(zip(raw_attrs[::2], raw_attrs[1::2]))
            if description["node"]["nodeName"].lower() != bound["tag"] or any(attrs.get(key) != value for key, value in bound["attributes"].items()):
                raise StaleElement()
            element = await page.get_element(bound["backendNodeId"])
            if kind == "click":
                await element.click()
            elif kind == "fill":
                await element.fill(action["text"])
            else:
                await element.set_input_files([request["uploadPath"]])
        elif kind == "key":
            await page.press(action["key"])
        elif kind == "scroll":
            mouse = await page.mouse
            await mouse.scroll(delta_x=action["deltaX"], delta_y=action["deltaY"])
        elif kind == "tab":
            if action["action"] == "open":
                page = await session.new_page(action.get("url") or "about:blank")
                await session.cdp_client.send.Target.activateTarget({"targetId": page._target_id})
            elif action["action"] == "close":
                tab = action.get("tabId") or target_id
                pages = await session.get_pages()
                if not any(value._target_id == tab for value in pages):
                    raise StaleElement()
                await session.close_page(tab)
                page = await session.must_get_current_page()
            else:
                tab = action.get("tabId")
                if not tab:
                    raise StaleElement()
                event = session.event_bus.dispatch(SwitchTabEvent(target_id=tab))
                await event
                await event.event_result(raise_if_any=True, raise_if_none=False)
                page = await session.must_get_current_page()
        else:
            raise RuntimeError("Unsupported private browser operation")
        info = await page.get_target_info()
        return {"page": {"targetId": info["targetId"], "url": info.get("url", ""), "title": info.get("title", "")}, "elements": []}
    finally:
        await asyncio.wait_for(session.stop(), STOP_TIMEOUT_SECONDS)


async def main():
    line = sys.stdin.buffer.readline(65537)
    if len(line) > 65536:
        raise RuntimeError("Browser request capacity exceeded")
    request = json.loads(line)
    if request["action"]["kind"] in ("state", "screenshot"):
        result = await observe(request)
    else:
        result = await act(request)
    return {"kind": "success", "value": result}


if __name__ == "__main__":
    try:
        result = asyncio.run(main())
    except StaleElement:
        result = {"kind": "failure", "value": {"reason": "staleElement"}}
    except Exception:
        # Driver exception text can contain cookies, URLs, typed text or CDP nonces.
        result = {"kind": "failure", "value": {"reason": "operationFailed"}}
    RESULT.write(json.dumps(result, ensure_ascii=True) + "\n")
    RESULT.flush()
