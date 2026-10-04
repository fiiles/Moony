/* Shared shell for the style guide: sidebar (real IA from src/components/common/nav-config.ts),
   small helpers (modal, toast, segmented control, number formatting). Not production code. */
(function () {
  var IA = {
    overview: { id: "overview", label: "Přehled", icon: "layout-dashboard" },
    groups: [
      { id: "money", label: "Peníze", items: [
        { id: "bankAccounts", label: "Bankovní účty", icon: "landmark", count: 3 },
        { id: "budgets", label: "Rozpočty", icon: "piggy-bank" },
        { id: "cashflow", label: "Cashflow", icon: "chart-column" },
        { id: "cashflowPlanning", label: "Plánování cashflow", icon: "repeat" } ] },
      { id: "investments", label: "Investice", items: [
        { id: "stocks", label: "Akcie", icon: "trending-up" },
        { id: "crypto", label: "Krypto", icon: "bitcoin" },
        { id: "bonds", label: "Dluhopisy", icon: "file-text" },
        { id: "realEstate", label: "Nemovitosti", icon: "house" },
        { id: "otherAssets", label: "Ostatní aktiva", icon: "gem" } ] },
      { id: "liabilities", label: "Závazky a pojištění", items: [
        { id: "loans", label: "Úvěry", icon: "credit-card" },
        { id: "insurance", label: "Pojištění", icon: "shield" } ] },
      { id: "tools", label: "Nástroje", collapsed: true, items: [
        { id: "projection", label: "Projekce", icon: "chart-line" },
        { id: "stocksAnalysis", label: "Analýza akcií", icon: "tag" },
        { id: "stockMonitor", label: "Sledování akcií", icon: "chart-candlestick" },
        { id: "annuityCalculator", label: "Anuitní kalkulačka", icon: "calculator" },
        { id: "estateCalculator", label: "Investice do nemovitosti", icon: "building" } ] }
    ],
    settings: { id: "settings", label: "Nastavení", icon: "settings" }
  };

  function item(it, active) {
    var tag = it.url ? "a" : "button";
    var cls = "nav__item" + (it.id === active ? " is-active" : "");
    var count = it.count ? '<span class="nav__count">' + it.count + "</span>" : "";
    var href = it.url ? ' href="' + it.url + '"' : ' type="button"';
    return "<" + tag + ' class="' + cls + '"' + href + ' data-nav="' + it.id + '">' + icon(it.icon) + "<span>" + it.label + "</span>" + count + "</" + tag + ">";
  }

  window.renderShell = function (opts) {
    var active = opts.active;
    var html = '<div class="brand"><i class="moon"></i><div><b>moony</b><small>Osobní finance</small></div></div>' +
      '<div class="sidebar__scroll"><div class="nav-group"><div class="nav">' + item(IA.overview, active) + "</div></div>";
    IA.groups.forEach(function (g) {
      var isActiveInside = g.items.some(function (i) { return i.id === active; });
      var collapsed = g.collapsed && !isActiveInside;
      html += '<div class="nav-group' + (collapsed ? " is-collapsed" : "") + '"><button type="button" class="nav-group__title" data-group="' + g.id + '"><span>' + g.label + "</span>" + icon("chevron-down", 12) + '</button><div class="nav">' +
        g.items.map(function (i) { return item(i, active); }).join("") + "</div></div>";
    });
    html += '</div><div class="sidebar__foot"><div class="nav">' + item(IA.settings, active) + "</div>" +
      '<button type="button" class="account"><i class="avatar">JN</i><div><b>Jana Nováková</b><small>Účet a zámek aplikace</small></div></button></div>';
    var aside = document.querySelector(".sidebar");
    aside.innerHTML = html;
    aside.addEventListener("click", function (e) {
      var g = e.target.closest(".nav-group__title");
      if (g) { g.parentElement.classList.toggle("is-collapsed"); return; }
      var b = e.target.closest("button.nav__item");
      if (b) { aside.querySelectorAll(".nav__item").forEach(function (n) { n.classList.remove("is-active"); }); b.classList.add("is-active"); }
    });
  };

  /* Segmented controls: any .seg toggles its own buttons; emits "segchange" with value. */
  document.addEventListener("click", function (e) {
    var b = e.target.closest(".seg__btn"); if (!b) return;
    var seg = b.closest(".seg");
    seg.querySelectorAll(".seg__btn").forEach(function (x) { x.classList.remove("is-active"); });
    b.classList.add("is-active");
    seg.dispatchEvent(new CustomEvent("segchange", { detail: b.dataset.value || b.textContent.trim(), bubbles: true }));
  });

  window.openModal = function (id) { document.getElementById(id).classList.add("is-open"); var f = document.getElementById(id).querySelector("input,select,textarea"); if (f) setTimeout(function () { f.focus(); }, 60); };
  window.closeModal = function (id) { document.getElementById(id).classList.remove("is-open"); };
  document.addEventListener("keydown", function (e) { if (e.key === "Escape") document.querySelectorAll(".modal-scrim.is-open").forEach(function (m) { m.classList.remove("is-open"); }); });
  document.addEventListener("click", function (e) { if (e.target.classList.contains("modal-scrim")) e.target.classList.remove("is-open"); });

  var toastTimer;
  window.toast = function (msg) {
    var t = document.querySelector(".toast");
    if (!t) { t = document.createElement("div"); t.className = "toast"; document.body.appendChild(t); }
    t.innerHTML = icon("check", 14) + "<span>" + msg + "</span>";
    requestAnimationFrame(function () { t.classList.add("is-on"); });
    clearTimeout(toastTimer); toastTimer = setTimeout(function () { t.classList.remove("is-on"); }, 2600);
  };

  /* Czech money formatting: thin non-breaking spaces as thousands separators, comma decimals. */
  window.fmtKc = function (n, dec) {
    dec = dec == null ? 0 : dec;
    var s = Math.abs(n).toFixed(dec).replace(".", ",");
    var parts = s.split(","); parts[0] = parts[0].replace(/\B(?=(\d{3})+(?!\d))/g, " ");
    return (n < 0 ? "− " : "") + parts.join(",") + " Kč";
  };
  window.fmtSigned = function (n, dec) { return (n >= 0 ? "+ " : "− ") + fmtKc(Math.abs(n), dec); };
  window.fmtPct = function (p, dec) { dec = dec == null ? 1 : dec; return (p >= 0 ? "+ " : "− ") + Math.abs(p).toFixed(dec).replace(".", ",") + " %"; };
  window.fmtDay = function (d) { return d.getDate() + ". " + (d.getMonth() + 1) + ". " + d.getFullYear(); };
  window.CZ_MONTHS = ["led", "úno", "bře", "dub", "kvě", "čvn", "čvc", "srp", "zář", "říj", "lis", "pro"];
})();
