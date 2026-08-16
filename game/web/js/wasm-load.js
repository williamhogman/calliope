// Version-locked WASM loader.
//
// The wasm-bindgen glue (calliope.js) and the binary (calliope_bg.wasm) are
// two halves of one artifact: the wasm's import section must match the glue
// exactly, or instantiation dies with "function import requires a callable".
// Plain HTTP caching can hand the browser a stale half after a rebuild, so
// both are fetched under the same build stamp — the cache can only ever
// serve a matching pair.
//
// E6.7 — one compile, two instantiations: the main thread compiles the
// binary once (streaming where the host cooperates) and hands the compiled
// `WebAssembly.Module` to the simulation worker over postMessage; both the
// worker's engine and the main-thread Orbital instantiate from that single
// module instead of fetching and compiling 3 MB twice.
import { WASM_V } from "./wasm/version.js";

/** Compile the wasm binary once per context; returns a WebAssembly.Module. */
let modulePromise = null;
export function loadModule() {
  modulePromise ??= (async () => {
    const url = new URL(`./wasm/calliope_bg.wasm?v=${WASM_V}`, import.meta.url);
    try {
      // E6.8 — streaming compile; requires the application/wasm MIME that
      // scripts/serve.py and the production host both send.
      return await WebAssembly.compileStreaming(fetch(url));
    } catch {
      // Host without the MIME type (or ancient browser): buffer compile.
      const buf = await (await fetch(url)).arrayBuffer();
      return WebAssembly.compile(buf);
    }
  })();
  return modulePromise;
}

let enginePromise = null;

/**
 * Load and initialise the WASM engine once; returns the module namespace.
 * Pass a precompiled `WebAssembly.Module` (E6.7) to skip this context's
 * own fetch+compile; without one the loader compiles locally.
 */
export function loadEngine(module) {
  enginePromise ??= (async () => {
    const mod = await import(`./wasm/calliope.js?v=${WASM_V}`);
    await mod.default({ module_or_path: module ?? (await loadModule()) });
    return mod;
  })();
  return enginePromise;
}
