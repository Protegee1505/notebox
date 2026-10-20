import init, { run } from "./pkg/opfs_spike.js";

let started;

async function start() {
  if (!started) {
    started = init();
  }
  await started;
}

self.addEventListener("message", async (event) => {
  const startedAt = performance.now();
  try {
    await start();
    const raw = await run(Boolean(event.data && event.data.reset));
    const elapsedMs = Math.round(performance.now() - startedAt);
    self.postMessage({ ok: true, elapsedMs, ...JSON.parse(raw) });
  } catch (error) {
    self.postMessage({
      ok: false,
      elapsedMs: Math.round(performance.now() - startedAt),
      error: String((error && error.stack) || error),
    });
  }
});
