// Google Analytics 4 for the hosted page only (shengtuo.me): page views and a few game events.
// main.js loads this after the game's first paint and calls `start`, then `track` per event. Nothing
// here may stop the game: every step is wrapped, and when the page is elsewhere (127.0.0.1 for
// `just web-serve` and `just web-shots`), or Google's script is blocked, `track` does nothing.
// `?analytics=debug` turns the events on anywhere without loading Google's script: they only go
// into `window.dataLayer`, where the page checks read them.

const MEASUREMENT_ID = "G-ZMH6NLL7C7";
// The only absolute URL in the page's scripts (web_check.mjs checks it).
const GTAG_URL = "https://www.googletagmanager.com/gtag/js";
// The site, and its GitHub Pages address, which redirects there.
const HOSTS = ["shengtuo.me", "h1994st.github.io"];

let on = false;

// Turns analytics on if the page `page` ({hostname, search}, usually `location`) is the site, or asks
// for the debug mode. Returns whether it is on.
export function start(page) {
  try {
    const debug = new URLSearchParams(page.search).get("analytics") === "debug";
    if (!debug && !HOSTS.includes(page.hostname)) return false;
    window.dataLayer = window.dataLayer || [];
    window.gtag = function gtag() {
      // gtag.js reads the arguments object itself, as Google's snippet pushes it.
      window.dataLayer.push(arguments);
    };
    window.gtag("js", new Date());
    window.gtag("config", MEASUREMENT_ID, { content_group: "chromatron" });
    on = true;
    if (!debug) {
      const script = document.createElement("script");
      script.async = true;
      script.src = `${GTAG_URL}?id=${MEASUREMENT_ID}`;
      // Blocked (an ad blocker, no network): events stop being queued.
      script.addEventListener("error", () => {
        on = false;
      });
      document.head.append(script);
    }
  } catch {
    on = false;
  }
  return on;
}

// Sends event `name` with `params`; nothing when analytics is off.
export function track(name, params) {
  if (!on) return;
  try {
    window.gtag("event", name, params);
  } catch {
    // Analytics never stops the game.
  }
}
