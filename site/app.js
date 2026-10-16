// Fills the download buttons with the latest release's installers and
// highlights the visitor's system. Without JavaScript or the GitHub API,
// every button still opens the latest release page.
(function () {
  var REPO = "ljamov/Stackyard";

  function detectOs() {
    var p = ((navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "").toLowerCase();
    var ua = navigator.userAgent.toLowerCase();
    if (p.indexOf("win") === 0 || ua.indexOf("windows") !== -1) return "windows";
    if (p.indexOf("mac") === 0 || ua.indexOf("mac os") !== -1) return "mac";
    if (p.indexOf("linux") !== -1 || ua.indexOf("linux") !== -1) return ua.indexOf("android") !== -1 ? null : "linux";
    return null;
  }

  var os = detectOs();
  var primaryAsset = { windows: "setup.exe", mac: "aarch64.dmg", linux: ".AppImage" };
  var primaryLabel = { windows: "Download for Windows", mac: "Download for macOS", linux: "Download for Linux" };

  if (os) {
    var card = document.querySelector('.platform[data-os="' + os + '"]');
    if (card) card.classList.add("mine");
    document.querySelector("[data-primary-label]").textContent = primaryLabel[os];
    if (os === "mac") document.querySelector("[data-primary-note]").textContent = "Apple silicon · Intel Macs: see other systems below";
  }

  fetch("https://api.github.com/repos/" + REPO + "/releases/latest", { headers: { Accept: "application/vnd.github+json" } })
    .then(function (r) { return r.ok ? r.json() : Promise.reject(r.status); })
    .then(function (release) {
      var assets = release.assets || [];
      function find(suffix) {
        for (var i = 0; i < assets.length; i++) if (assets[i].name.slice(-suffix.length) === suffix) return assets[i];
        return null;
      }
      function mb(bytes) { return (bytes / 1048576).toFixed(0) + " MB"; }

      document.querySelectorAll("[data-asset]").forEach(function (a) {
        var asset = find(a.getAttribute("data-asset"));
        if (!asset) return;
        a.href = asset.browser_download_url;
        a.title = asset.name + " · " + mb(asset.size);
      });

      if (os) {
        var main = find(primaryAsset[os]);
        if (main) {
          var btn = document.querySelector("[data-primary]");
          btn.href = main.browser_download_url;
          btn.title = main.name + " · " + mb(main.size);
        }
      }

      var v = release.tag_name || "";
      document.querySelectorAll("[data-version]").forEach(function (el) { el.textContent = "Latest: " + v; });
      document.querySelectorAll("[data-version-short]").forEach(function (el) { el.textContent = v; });
    })
    .catch(function () { /* buttons keep pointing at the releases page */ });

  // Screenshot tabs
  var img = document.getElementById("tour-img");
  var alts = {
    sites: "The Sites screen with three local HTTPS sites",
    dashboard: "Dashboard with services, sites and runtimes",
    runtimes: "Runtimes screen with PHP and Node.js versions",
    databases: "Databases screen listing tables",
    logs: "Logs screen with warnings and errors highlighted"
  };
  document.querySelectorAll(".tabs button").forEach(function (b) {
    b.addEventListener("click", function () {
      document.querySelectorAll(".tabs button").forEach(function (o) { o.setAttribute("aria-selected", o === b ? "true" : "false"); });
      var name = b.getAttribute("data-shot");
      img.style.opacity = "0.3";
      var next = new Image();
      next.onload = function () { img.src = next.src; img.alt = alts[name]; img.style.opacity = "1"; };
      next.src = "images/" + name + ".png";
    });
  });
})();
