// Fetches Typst packages from packages.typst.org, cached in IndexedDB.

const DB_NAME = "cetz-editor";
const STORE = "packages";

/** `@preview/cetz:0.5.2` → `https://packages.typst.org/preview/cetz-0.5.2.tar.gz` */
export function packageUrl(spec: string): string {
  const m = /^@([a-z0-9-]+)\/([a-zA-Z0-9_-]+):(\d+\.\d+\.\d+)$/.exec(spec);
  if (!m) throw new Error(`invalid package spec: ${spec}`);
  const [, namespace, name, version] = m;
  if (namespace !== "preview") {
    throw new Error(`only @preview packages can be fetched (got ${spec})`);
  }
  return `https://packages.typst.org/${namespace}/${name}-${version}.tar.gz`;
}

export async function loadPackage(spec: string): Promise<Uint8Array> {
  const cached = await cacheGet(spec);
  if (cached) return cached;
  const res = await fetch(packageUrl(spec));
  if (!res.ok) throw new Error(`failed to fetch ${spec}: HTTP ${res.status}`);
  const bytes = new Uint8Array(await res.arrayBuffer());
  await cachePut(spec, bytes);
  return bytes;
}

// The cache is best-effort: IndexedDB can be unavailable (private windows,
// blocked storage), in which case packages are simply refetched.

let db: Promise<IDBDatabase | null> | undefined;

function openDb(): Promise<IDBDatabase | null> {
  db ??= new Promise((resolve) => {
    try {
      const req = indexedDB.open(DB_NAME, 1);
      req.onupgradeneeded = () => req.result.createObjectStore(STORE);
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => resolve(null);
    } catch {
      resolve(null);
    }
  });
  return db;
}

async function cacheGet(spec: string): Promise<Uint8Array | undefined> {
  const conn = await openDb();
  if (!conn) return undefined;
  return new Promise((resolve) => {
    try {
      const req = conn.transaction(STORE).objectStore(STORE).get(spec);
      req.onsuccess = () => resolve(req.result instanceof Uint8Array ? req.result : undefined);
      req.onerror = () => resolve(undefined);
    } catch {
      resolve(undefined);
    }
  });
}

async function cachePut(spec: string, bytes: Uint8Array): Promise<void> {
  const conn = await openDb();
  if (!conn) return;
  return new Promise((resolve) => {
    try {
      const tx = conn.transaction(STORE, "readwrite");
      tx.objectStore(STORE).put(bytes, spec);
      tx.oncomplete = () => resolve();
      tx.onerror = () => resolve();
    } catch {
      resolve();
    }
  });
}
