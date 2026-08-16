// Version-locked WASM loader.
//
// The wasm-bindgen glue (calliope.js) and the binary (calliope_bg.wasm) are
// two halves of one artifact: the wasm's import section must match the glue
// exactly, or instantiation dies with "function import requires a callable".
// Plain HTTP caching can hand the browser a stale half after a rebuild, so
// both are fetched under the same build stamp — the cache can only ever
// serve a matching pair.
import { WASM_V } from "./wasm/version.js";

let enginePromise = null;

/** Load and initialise the WASM engine once; returns the module namespace. */
export function loadEngine() {
  enginePromise ??= (async () => {
    const mod = await import(`./wasm/calliope.js?v=${WASM_V}`);
    await mod.default({
      module_or_path: new URL(`./wasm/calliope_bg.wasm?v=${WASM_V}`, import.meta.url),
    });
    return mod;
  })();
  return enginePromise;
}
