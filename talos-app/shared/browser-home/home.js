function updateClock() {
  const now = new Date();
  document.querySelector("#clock").textContent = now.toLocaleTimeString([], {hour: "2-digit", minute: "2-digit", hour12: false});
  document.querySelector("#clock").dateTime = now.toISOString();
  document.querySelector("#date").textContent = now.toLocaleDateString([], {weekday: "long", month: "long", day: "numeric"}) + " · " + Intl.DateTimeFormat().resolvedOptions().timeZone;
}
updateClock();
setInterval(updateClock, 1000);
document.querySelector("#navigate").addEventListener("submit", (event) => {
  event.preventDefault();
  try {
    const address = document.querySelector("#address").value.trim();
    const url = new URL(address.includes("://") ? address : `https://${address}`);
    if (!["https:", "http:"].includes(url.protocol)) throw new Error("Unsupported address");
    location.assign(url.href);
  } catch {
    document.querySelector("#error").textContent = "Enter an HTTP or HTTPS website address.";
  }
});
