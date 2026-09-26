document.addEventListener("DOMContentLoaded", () => {
  const current = document.documentElement.lang === "es" ? "es" : "en";
  // mdBook generates each language in its own directory. Derive the site root
  // from the URL itself rather than resolving ../ links against the current page.
  const location = window.location.pathname.match(/^(.*)\/(en|es)\/(.*)$/);
  if (!location || location[2] !== current) return;

  const [, siteRoot, , page] = location;
  const chapter = page || "index.html";
  const toolbar = document.querySelector("#mdbook-menu-bar .right-buttons");
  if (!toolbar) return;

  const switcher = document.createElement("nav");
  switcher.className = "language-switch";
  switcher.setAttribute("aria-label", current === "es" ? "Idioma" : "Language");

  for (const language of ["en", "es"]) {
    const link = document.createElement("a");
    link.href = `${siteRoot}/${language}/${chapter}`;
    link.textContent = language.toUpperCase();
    link.lang = language;
    link.hreflang = language;
    if (language === current) link.setAttribute("aria-current", "page");
    switcher.appendChild(link);
  }

  toolbar.prepend(switcher);
});
