// Renders trailer.html frame by frame in headless Edge (deterministic: the page
// draws any moment with window.render(t)) and pipes the frames into FFmpeg.
// usage: node render.mjs stills <outDir> <t1,t2,...>
//        node render.mjs video <out.mp4> [fps] [from] [to]
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, extname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..");
const FFMPEG = join(repo, "third_party", "ffmpeg", "bin", "ffmpeg.exe");
const EDGE = process.env.VIDO_EDGE ?? "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe";
const [mode, out, a3, a4, a5] = process.argv.slice(2);

// URL path → file. Generated frames live in build/, fonts and the logo are the app's own.
const ROUTES = {
  "/fonts/geist.woff2": join(repo, "node_modules/@fontsource-variable/geist/files/geist-latin-wght-normal.woff2"),
  "/fonts/geist-mono.woff2": join(repo, "node_modules/@fontsource-variable/geist-mono/files/geist-mono-latin-wght-normal.woff2"),
  "/assets/icon.png": join(repo, "config/icons/icon.png"),
};
function resolve(path) {
  if (ROUTES[path]) return ROUTES[path];
  if (path.startsWith("/assets/")) return join(here, "build", path);
  return join(here, path);
}
const types = { ".html": "text/html", ".png": "image/png", ".woff2": "font/woff2" };
const server = createServer((req, res) => {
  const p = resolve(decodeURIComponent(new URL(req.url, "http://x").pathname));
  if (!p.startsWith(repo) || !existsSync(p)) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { "content-type": types[extname(p)] ?? "application/octet-stream" }).end(readFileSync(p));
});
await new Promise((r) => server.listen(5288, "127.0.0.1", r));

const port = 9362;
const edge = spawn(
  EDGE,
  ["--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${join(process.env.TEMP, "vido-trailer-render")}`, "--hide-scrollbars", "--force-device-scale-factor=1", "about:blank"],
  { stdio: "ignore" },
);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let targets;
for (let i = 0; i < 60; i++) {
  try {
    targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
    if (targets.some((x) => x.type === "page")) break;
  } catch {}
  await sleep(200);
}
const ws = new WebSocket(targets.find((x) => x.type === "page").webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let id = 0;
const pending = new Map();
ws.onmessage = (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pending.has(d.id)) {
    pending.get(d.id)(d);
    pending.delete(d.id);
  }
};
const send = (method, params = {}) =>
  new Promise((r) => {
    const i = ++id;
    pending.set(i, r);
    ws.send(JSON.stringify({ id: i, method, params }));
  });
const ev = async (e) => {
  const r = await send("Runtime.evaluate", { expression: e, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails).slice(0, 400));
  return r.result?.result?.value;
};
await send("Page.enable");
await send("Emulation.setDeviceMetricsOverride", { width: 1920, height: 1080, deviceScaleFactor: 1, mobile: false });
await send("Page.navigate", { url: "http://127.0.0.1:5288/trailer.html" });
await sleep(1500);
await ev(`(async () => { await document.fonts.ready; await Promise.all([...document.images].map((i) => i.complete ? 0 : new Promise((r) => { i.onload = i.onerror = r; }))); return true; })()`);
const duration = await ev("window.DURATION");
// Touch every scene once so images are decoded before the first captured frame.
for (let s = 0; s < duration; s += 1) await ev(`render(${s})`);

async function frame(time, format = "jpeg") {
  await ev(`render(${time}); new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))`);
  const r = await send("Page.captureScreenshot", { format, quality: format === "jpeg" ? 92 : undefined, optimizeForSpeed: true });
  return Buffer.from(r.result.data, "base64");
}

if (mode === "stills") {
  mkdirSync(out, { recursive: true });
  for (const s of a3.split(",")) {
    writeFileSync(join(out, `t${String(s).padStart(6, "0")}.png`), await frame(Number(s), "png"));
  }
  console.log(`stills: ${out}`);
} else {
  const fps = Number(a3 ?? 30);
  const from = Number(a4 ?? 0);
  const to = Number(a5 ?? duration);
  const ff = spawn(FFMPEG, ["-hide_banner", "-loglevel", "error", "-y", "-f", "image2pipe", "-framerate", String(fps), "-i", "-",
    "-c:v", "libx264", "-preset", "slow", "-crf", "17", "-pix_fmt", "yuv420p", "-movflags", "+faststart", out], { stdio: ["pipe", "inherit", "inherit"] });
  const n = Math.round((to - from) * fps);
  const started = Date.now();
  for (let i = 0; i < n; i++) {
    const buf = await frame(from + i / fps);
    if (!ff.stdin.write(buf)) await new Promise((r) => ff.stdin.once("drain", r));
    if (i % 300 === 0) console.log(`frame ${i}/${n} (${((Date.now() - started) / 1000).toFixed(0)} s)`);
  }
  ff.stdin.end();
  await new Promise((r) => ff.on("close", r));
  console.log(`video: ${out}`);
}
ws.close();
edge.kill();
server.close();
process.exit(0);
