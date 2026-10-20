// OPFS image store for the browser build.
//
// The Rust side keeps SQLite in memory and sends a versioned database image
// here after writes. One worker owns one file and one exclusive Web Lock, so
// a second tab cannot silently overwrite the first tab's notes.
//
// Messages in:  { type: "load" }
//               { type: "save", bytes: ArrayBuffer }
// Messages out: { type: "loaded", bytes: ArrayBuffer, persisted: boolean }
//               { type: "locked" }
//               { type: "unavailable", message: string }
//               { type: "saved", bytes: number, ms: number }
//               { type: "error", message: string }

const FILE_NAME = "notebox.db";
const LOCK_NAME = "notebox-opfs-store-v1";

let ready = null;
let locked = false;
let persisted = false;
let persistRequest = null;
let writeChain = Promise.resolve();

async function fileHandle() {
  const root = await navigator.storage.getDirectory();
  return root.getFileHandle(FILE_NAME, { create: true });
}

async function init() {
  if (ready) return ready;
  ready = (async () => {
    // Hold the lock for the life of the worker. The callback's promise never
    // settles on purpose; only stopping the worker releases it.
    if (navigator.locks && navigator.locks.request) {
      const acquired = await new Promise((resolve) => {
        navigator.locks
          .request(LOCK_NAME, { mode: "exclusive", ifAvailable: true }, (lock) => {
            resolve(Boolean(lock));
            if (lock) return new Promise(() => {});
            return undefined;
          })
          .catch(() => resolve(true));
      });
      if (!acquired) {
        locked = true;
        return;
      }
    }
    try {
      if (navigator.storage && navigator.storage.persist) {
        // Do not hold the boot screen on a permission prompt. The load path
        // waits at most 500 ms for a quick answer and otherwise reports the
        // conservative "may be cleared" copy.
        persistRequest = navigator.storage
          .persist()
          .then((value) => {
            persisted = Boolean(value);
          })
          .catch(() => {});
      }
    } catch (_) {
      // A denied or missing persistence request only means eviction is
      // possible. The data is still written to OPFS.
    }
  })();
  return ready;
}

async function load() {
  await init();
  if (locked) {
    self.postMessage({ type: "locked" });
    return;
  }
  try {
    const handle = await fileHandle();
    const file = await handle.getFile();
    if (persistRequest) {
      await Promise.race([
        persistRequest,
        new Promise((resolve) => setTimeout(resolve, 500)),
      ]);
    }
    if (file.size === 0) {
      self.postMessage({
        type: "loaded",
        bytes: new ArrayBuffer(0),
        persisted,
      });
      return;
    }
    const bytes = await file.arrayBuffer();
    self.postMessage({ type: "loaded", bytes, persisted }, [bytes]);
  } catch (error) {
    self.postMessage({
      type: "unavailable",
      message: String((error && error.message) || error),
    });
  }
}

function save(bytes) {
  const write = async () => {
    const started = performance.now();
    const handle = await fileHandle();
    const writable = await handle.createWritable();
    await writable.write(bytes);
    await writable.close();
    self.postMessage({
      type: "saved",
      bytes: bytes.byteLength || 0,
      ms: Math.round(performance.now() - started),
    });
  };
  writeChain = writeChain.then(write, write);
  writeChain = writeChain.catch((error) => {
    self.postMessage({
      type: "error",
      message: String((error && error.message) || error),
    });
  });
}

self.onmessage = (event) => {
  const data = event.data || {};
  if (data.type === "load") {
    load();
    return;
  }
  if (data.type === "save" && data.bytes) {
    const bytes =
      data.bytes instanceof ArrayBuffer ? data.bytes : data.bytes.buffer;
    save(bytes);
  }
};
