// Preserve published homepage anchors and source shares after the page split.
function followLegacyLink() {
  if (document.body.dataset.page !== "home") return;
  const routes = {
    "#playground": "playground.html#playground",
    "#examples": "playground.html#examples",
    "#packages": "docs.html#packages",
    "#features": "docs.html#features",
  };
  const destination = location.hash.startsWith("#src=")
    ? "playground.html" + location.hash
    : routes[location.hash];
  if (!destination) return;
  const target = new URL(destination, location.href);
  target.search = location.search;
  location.replace(target);
}
followLegacyLink();
window.addEventListener("hashchange", followLegacyLink);
