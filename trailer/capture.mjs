// UI screenshots for the trailer, taken from trailer/mock.html (the real UI on a
// fake backend) in headless Edge over CDP.
// usage: node capture.mjs <outDir> <steps.json>  (Vite must serve the repo on :5199)
// steps: {click: text} {clickSel: css} {eval: js} {key} {wait: ms} {shot: file}
//        {drag: [x1,y1,x2,y2]} {dragJs: "js returning [x1,y1,x2,y2]"} {reload: true}
//        {pick: [comboboxIndexInContainer css, optionText]}
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [out, stepsFile] = process.argv.slice(2);
mkdirSync(out, { recursive: true });
const steps = JSON.parse(readFileSync(stepsFile, "utf8"));
const port = 9351;
const edge = spawn(
  process.env.VIDO_EDGE ?? "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  ["--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${join(process.env.TEMP, "vido-trailer-cap")}`, "--hide-scrollbars", "--mute-audio", "--autoplay-policy=no-user-gesture-required", "about:blank"],
  { stdio: "ignore" },
);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let t;
for (let i = 0; i < 50; i++) {
  try {
    t = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
    if (t.some((x) => x.type === "page")) break;
  } catch {}
  await sleep(200);
}
const ws = new WebSocket(t.find((x) => x.type === "page").webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let id = 0;
const p = new Map();
ws.onmessage = (m) => {
  const d = JSON.parse(m.data);
  if (d.id && p.has(d.id)) {
    p.get(d.id)(d);
    p.delete(d.id);
  }
};
const send = (method, params = {}) =>
  new Promise((r) => {
    const i = ++id;
    p.set(i, r);
    ws.send(JSON.stringify({ id: i, method, params }));
  });
const ev = async (e) => {
  const r = await send("Runtime.evaluate", { expression: e, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) console.error("eval", JSON.stringify(r.result.exceptionDetails).slice(0, 300));
  return r.result?.result?.value;
};
const mouse = (type, x, y) =>
  send("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons: type === "mouseReleased" ? 0 : 1, clickCount: 1, pointerType: "mouse" });
async function drag([x1, y1, x2, y2]) {
  await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: x1, y: y1, pointerType: "mouse" });
  await mouse("mousePressed", x1, y1);
  for (let k = 1; k <= 12; k++) {
    await mouse("mouseMoved", x1 + ((x2 - x1) * k) / 12, y1 + ((y2 - y1) * k) / 12);
    await sleep(16);
  }
  await mouse("mouseReleased", x2, y2);
}
await send("Page.enable");
await send("Emulation.setDeviceMetricsOverride", { width: 1920, height: 1080, deviceScaleFactor: 1, mobile: false });
await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });
await send("Page.navigate", { url: "http://localhost:5199/trailer/mock.html" });
await sleep(4500);
for (const s of steps) {
  if (s.reload) {
    await send("Page.reload");
    await sleep(4000);
  }
  if (s.click)
    console.log("click", s.click, await ev(`(() => { const b = [...document.querySelectorAll("button,[role=menuitem],[role=option],[role=radio]")].find((b) => b.textContent.trim() === ${JSON.stringify(s.click)}); b?.click(); return !!b; })()`));
  if (s.clickSel) console.log("clickSel", s.clickSel, await ev(`(() => { const b = document.querySelector(${JSON.stringify(s.clickSel)}); b?.click(); return !!b; })()`));
  if (s.pick) {
    const [sel, text] = s.pick;
    await ev(`(() => { const b = document.querySelector(${JSON.stringify(sel)}); b.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,button:0,pointerType:'mouse'})); })()`);
    await sleep(400);
    console.log("pick", text, await ev(`(() => { const o = [...document.querySelectorAll('[role=option]')].find((o) => o.textContent.trim() === ${JSON.stringify(text)}); o?.click(); return !!o; })()`));
  }
  if (s.eval) console.log("eval", await ev(s.eval));
  if (s.drag) await drag(s.drag);
  if (s.dragJs) {
    const c = await ev(s.dragJs);
    console.log("drag", c);
    if (c) await drag(c);
  }
  if (s.key) {
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: s.key });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: s.key });
  }
  await sleep(s.wait ?? 700);
  if (s.shot) {
    const r = await send("Page.captureScreenshot", { format: "png" });
    writeFileSync(join(out, s.shot), Buffer.from(r.result.data, "base64"));
    console.log("shot", s.shot);
  }
}
ws.close();
edge.kill();
process.exit(0);
