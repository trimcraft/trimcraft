// README screenshots (docs/images) of the browser demo, taken with headless Chrome over the
// DevTools protocol. Start the demo first (npm run dev), then:
//   node scripts/screenshots.mjs docs/images http://localhost:1420 virtual-controls connections hardware settings credits overlay
import { spawn } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [outDir, base, ...wanted] = process.argv.slice(2);
const CHROME = process.env.CHROME ?? "C:/Program Files/Google/Chrome/Application/chrome.exe";
const PORT = 9347;
const W = 1280, H = 860;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const profile = mkdtempSync(join(tmpdir(), "shots-"));
const chrome = spawn(CHROME, [
  "--headless=new", `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`,
  `--window-size=${W},${H}`, "--hide-scrollbars", "--no-first-run", "about:blank",
], { stdio: "ignore" });

let targets;
for (let i = 0; i < 50; i++) {
  try { targets = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json(); break; } catch { await sleep(200); }
}
const page = targets.find((t) => t.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r, { once: true }));
let id = 0;
const pending = new Map();
ws.addEventListener("message", (e) => {
  const m = JSON.parse(e.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
});
const send = (method, params = {}) => new Promise((resolve, reject) => {
  const n = ++id;
  pending.set(n, (m) => (m.error ? reject(new Error(`${method}: ${m.error.message}`)) : resolve(m.result)));
  ws.send(JSON.stringify({ id: n, method, params }));
});
const js = async (expr) => {
  const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  return r.result.value;
};
const click = (text, sel = "button, a") => js(`(() => {
  const el = [...document.querySelectorAll(${JSON.stringify(sel)})].find((b) => b.textContent.trim() === ${JSON.stringify(text)} || b.getAttribute("aria-label") === ${JSON.stringify(text)} || b.title === ${JSON.stringify(text)});
  if (!el) throw new Error("not found: " + ${JSON.stringify(text)});
  el.click(); return true; })()`);

await send("Page.enable");
await send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: 1, mobile: false });
await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });

async function open(path, section) {
  await send("Page.navigate", { url: base + path });
  await sleep(1500);
  if (section !== undefined) {
    await js(`localStorage.setItem("trimcraft.section", ${JSON.stringify(section)})`);
    await send("Page.reload");
    await sleep(2500);
  }
}
async function shot(name, clip) {
  // The demo pretends a game runs as administrator; that warning is not part of the picture.
  await js(`[...document.querySelectorAll("div, p")].filter((e) => /runs as administrator; Trimcraft/.test(e.textContent) && e.children.length <= 2).forEach((e) => e.remove())`);
  await sleep(300);
  const r = await send("Page.captureScreenshot", { format: "png", ...(clip ? { clip: { ...clip, scale: 1 } } : {}) });
  writeFileSync(join(outDir, `${name}.png`), Buffer.from(r.data, "base64"));
  console.log("wrote", name);
}

const SHOTS = {
  "virtual-controls": async () => {
    await open("/", "controls");
    await click("Activate");
    await sleep(800);
    await js(`(() => { const h = [...document.querySelectorAll("h2")].find((e) => /virtual controls/i.test(e.textContent)); h?.scrollIntoView(); window.scrollBy(0, -12); })()`);
    await sleep(500);
    await shot("virtual-controls");
  },
  connections: async () => {
    await open("/", "connections");
    await js(`[...document.querySelectorAll("summary, button")].filter((e) => /How to set up/.test(e.textContent)).forEach((e) => e.click())`);
    await sleep(500);
    await shot("connections");
  },
  hardware: async () => { await open("/", "hardware"); await sleep(2000); await shot("hardware-test"); },
  settings: async () => {
    await open("/", "controls");
    await click("Settings", "button");
    await sleep(800);
    await shot("settings");
  },
  credits: async () => {
    await open("/", "controls");
    await click("Settings", "button");
    await sleep(500);
    await click("About", "button");
    await sleep(500);
    await click("Credits", "button");
    await sleep(800);
    await shot("credits");
  },
  overlay: async () => {
    await open("/overlay.html");
    await sleep(3000);
    await shot("overlay", { x: 0, y: 0, width: 800, height: 600 });
  },
};

try {
  for (const w of wanted) await SHOTS[w]();
} finally {
  ws.close();
  chrome.kill();
}
