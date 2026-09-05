import { mkdir, writeFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
const port = Number(process.argv[2] || 9230);
const outputDirectory = 'dist/fullscreen-verification';
await mkdir(outputDirectory, {recursive: true});
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

let target;
for (let attempt = 0; attempt < 60; attempt += 1) {
  try {
    const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then((response) => response.json());
    target = targets.find((candidate) => candidate.type === "page" && candidate.url.includes("youtube.com"));
    if (target) break;
  } catch (_) {}
  await delay(1000);
}
if (!target) throw new Error("No se encontró YouTube");

const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.addEventListener("open", resolve, { once: true });
  socket.addEventListener("error", reject, { once: true });
});

let nextId = 1;
const pending = new Map();
socket.addEventListener("message", (event) => {
  const message = JSON.parse(event.data);
  if (!message.id || !pending.has(message.id)) return;
  const callbacks = pending.get(message.id);
  pending.delete(message.id);
  if (message.error) callbacks.reject(new Error(JSON.stringify(message.error)));
  else callbacks.resolve(message.result);
});
const command = (method, params = {}) => new Promise((resolve, reject) => {
  const id = nextId++;
  pending.set(id, { resolve, reject });
  socket.send(JSON.stringify({ id, method, params }));
});
const evaluate = async (expression) => {
  const result = await command("Runtime.evaluate", { expression, returnByValue: true });
  return result.result.value;
};
const inspect = () => evaluate(`({
  href: location.href,
  viewport: { width: innerWidth, height: innerHeight },
  screen: { width: screen.width, height: screen.height },
  fullscreen: Boolean(document.fullscreenElement),
  fullscreenTag: document.fullscreenElement?.tagName || null,
  video: (() => {const v=document.querySelector('video'); return v ? {time:v.currentTime, paused:v.paused, width:v.videoWidth, height:v.videoHeight, frames:v.getVideoPlaybackQuality().totalVideoFrames} : null;})(),
  readyState: document.readyState
})`);
const key = async (key, code, virtualKeyCode) => {
  await command("Input.dispatchKeyEvent", { type: "keyDown", key, code, windowsVirtualKeyCode: virtualKeyCode, nativeVirtualKeyCode: virtualKeyCode });
  await command("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: virtualKeyCode, nativeVirtualKeyCode: virtualKeyCode });
};

await delay(5000);
// Regression: an ad state on a media container must not remove the player.
await evaluate(`(() => {
  const node=document.createElement('div');
  node.id='croni-player-regression';
  node.className='html5-video-player ad-showing';
  node.hidden=true;
  node.appendChild(document.createElement('video'));
  document.body.appendChild(node);
})()`);
await delay(100);
assert.equal(await evaluate(`(() => {const n=document.getElementById('croni-player-regression'); const exists=Boolean(n); n?.remove(); return exists;})()`), true, 'Ad state must not delete video container');
await command('Runtime.evaluate', {expression: "document.querySelector('video')?.play()", userGesture:true, awaitPromise:true});
await delay(3000);
const before = await inspect();
await key("f", "KeyF", 70);
await delay(4000);
const entered = await inspect();
const screenshot = await command('Page.captureScreenshot', {format:'png'});
await writeFile(`${outputDirectory}/fullscreen.png`, Buffer.from(screenshot.data,'base64'));
await key("Escape", "Escape", 27);
await delay(4000);
const exited = await inspect();
const exitScreenshot=await command('Page.captureScreenshot', {format:'png'});
await writeFile(`${outputDirectory}/exited.png`, Buffer.from(exitScreenshot.data,'base64'));
console.log(JSON.stringify({ before, entered, exited }, null, 2));
assert.equal(entered.fullscreen, true);
assert.equal(exited.fullscreen, false);
assert.deepEqual(exited.viewport, before.viewport);
assert.ok(before.video?.width > 0, 'Video must exist with decoded image');
assert.ok(entered.video?.frames > before.video.frames, 'Frames must advance in fullscreen');
assert.ok(exited.video?.frames > entered.video.frames, 'Frames must advance after Escape');
await command('Page.reload');
await delay(8000);
await command('Runtime.evaluate', {expression: "document.querySelector('video')?.play()", userGesture:true, awaitPromise:true});
await delay(2000);
const reloaded=await inspect();
assert.equal(reloaded.fullscreen,false);
assert.ok(reloaded.video?.width>0,'Video must decode after reload');
assert.deepEqual(reloaded.viewport,before.viewport);
await writeFile(`${outputDirectory}/result.json`,JSON.stringify({before,entered,exited,reloaded},null,2));
console.log('PASS: player survives ad state, F, Escape and reload');
socket.close();
