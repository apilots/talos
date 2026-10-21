const bridge = window.talosBrowser;
const address = document.querySelector("#address");
let currentAddress = "";
address.addEventListener("blur", () => { address.value = currentAddress; });
for (const kind of ["back", "forward", "reload", "new", "hide"]) {
  document.querySelector(`#${kind}`).addEventListener("click", () => bridge.act({kind}));
}
document.querySelector("#address-form").addEventListener("submit", event => {
  event.preventDefault();
  const url = address.value.trim();
  address.blur();
  bridge.act({kind: "navigate", url});
});
bridge.subscribe(state => {
  if (state.focusAddress) { address.focus(); address.select(); return; }
  currentAddress = state.address;
  if (document.activeElement !== address) address.value = state.address;
  document.querySelector("#back").disabled = !state.back;
  document.querySelector("#forward").disabled = !state.forward;
  document.querySelector("#reload").classList.toggle("loading", state.loading);
  document.querySelector("#error").textContent = state.error || "";
  const list = document.querySelector("#list");
  const keep = new Set(state.tabs.map(tab => tab.id));
  for (const child of [...list.children]) if (!keep.has(child.dataset.id)) child.remove();
  for (const tab of state.tabs) {
    let container = [...list.children].find(child => child.dataset.id === tab.id);
    if (!container) {
      container = document.createElement("div"); container.dataset.id = tab.id;
      const focus = document.createElement("button"); focus.type = "button";
      focus.addEventListener("click", () => bridge.act({kind:"focus", tabId:tab.id}));
      const close = document.createElement("button"); close.type = "button"; close.textContent = "×";
      close.addEventListener("click", () => bridge.act({kind:"close", tabId:tab.id}));
      container.append(focus, close); list.append(container);
    }
    container.className = `tab${tab.active ? " active" : ""}`;
    const focus = container.children[0]; focus.textContent = tab.title; focus.title = tab.title;
    focus.setAttribute("aria-pressed", String(tab.active));
    container.children[1].setAttribute("aria-label", `Close ${tab.title}`);
  }
});
bridge.act({kind:"state"});
