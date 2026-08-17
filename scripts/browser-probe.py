#!/usr/bin/env python3
"""Browser-side performance gates (E10.5, E10.7) and the GL drill (E9.10).

Drives real Chromium against the dev server on :8080 and writes a banded
report in the same [PASS]/[WARN]/[FAIL] dialect as the native harness, so
SUMMARY.txt aggregates it like any other section.

  E10.5  boot probe      cold load -> engine ready -> first rendered frame
  E10.7  long-task audit PerformanceObserver over 100 months of speed-3 playback
  E9.10  GL drill        scripted WebGL context loss; recovery must land on a
                         fresh WebGL2 canvas (or hand off cleanly to the CPU
                         compositor) with the map still drawing

Usage: python3 scripts/browser-probe.py [out.txt]
Headless Chromium runs on a software rasteriser — the bands account for it.
"""

import asyncio
import sys
import time
from pathlib import Path

from playwright.async_api import async_playwright

URL = "http://localhost:8080/?seed=777&size=512"
OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("game/reports/browser.txt")

# (name, sweet, hard, target) — mirrors util::Band; lower is better unless noted
BANDS = {
    "boot to engine ready": (25.0, 60.0, "E10.5: cold load → world unpacked, loader faded (s)"),
    "ready to first frame": (2.0, 6.0, "E10.5: engine ready → first annotated draw (s)"),
    "long tasks in playback": (12.0, 60.0, "E10.7: main-thread tasks >50 ms across 100 months at speed 3"),
    "worst long task": (200.0, 800.0, "E10.7: single worst main-thread stall (ms)"),
}

rows = []
info = []


def check(name, value, shown):
    sweet, hard, target = BANDS[name]
    tag = "PASS" if value <= sweet else ("WARN" if value <= hard else "FAIL")
    rows.append(f"[{tag}] {name:<44} {shown:>12}   ({target})")
    return tag


def must(name, ok, detail):
    rows.append(f"[{'PASS' if ok else 'FAIL'}] {name:<44} {detail:>12}")
    return ok


async def main():
    async with async_playwright() as p:
        browser = await p.chromium.launch(headless=True)
        page = await (await browser.new_context(viewport={"width": 1280, "height": 900})).new_page()
        console = []
        page.on("console", lambda m: console.append(m.text))

        # ---------------- E10.5 boot probe ----------------
        t0 = time.monotonic()
        await page.goto(URL, wait_until="domcontentloaded")
        await page.wait_for_function(
            "document.getElementById('loading').classList.contains('fade')", timeout=120000
        )
        t_ready = time.monotonic() - t0
        await page.wait_for_function("(window.__calliope.draws || 0) >= 1", timeout=20000)
        t_frame = time.monotonic() - t0
        backend = await page.evaluate("window.__calliope.gpuBackend()")
        info.append(f"boot: ready {t_ready:.1f}s · first frame {t_frame:.1f}s · backend {backend}")
        check("boot to engine ready", t_ready, f"{t_ready:.1f} s")
        check("ready to first frame", max(0.0, t_frame - t_ready), f"{t_frame - t_ready:.2f} s")

        # ---------------- E10.7 long-task audit ----------------
        # 100 months at speed 3 (3 months/s), exactly as a player runs it.
        await page.evaluate("""() => {
          window.__lt = { n: 0, worst: 0 };
          new PerformanceObserver((list) => {
            for (const e of list.getEntries()) {
              window.__lt.n++;
              window.__lt.worst = Math.max(window.__lt.worst, e.duration);
            }
          }).observe({ entryTypes: ['longtask'] });
        }""")
        m0 = await page.evaluate("window.__calliope.month()")
        await page.evaluate("() => { window.__calliope.setSpeed(3); window.__calliope.playPause(); }")
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            m = await page.evaluate("window.__calliope.month()")
            if m - m0 >= 100:
                break
            await asyncio.sleep(1.0)
        months = await page.evaluate("window.__calliope.month()") - m0
        await page.evaluate("window.__calliope.playPause()")  # pause
        lt = await page.evaluate("window.__lt")
        info.append(f"playback: {months} months at speed 3 · {lt['n']} long tasks · worst {lt['worst']:.0f} ms")
        check("long tasks in playback", float(lt["n"]), f"{lt['n']}")
        check("worst long task", float(lt["worst"]), f"{lt['worst']:.0f} ms")

        # ---------------- E9.10 context-loss drill ----------------
        # Kill the WebGL context under the engine mid-flight; the recovery
        # path must bring up a fresh WebGL2 canvas and keep the map drawing.
        pre = await page.evaluate("""() => ({
          backend: window.__calliope.gpuBackend(),
          gl: !!document.getElementById('gl'),
        })""")
        if pre["backend"] == "cpu" or not pre["gl"]:
            info.append("GL drill: no live GL canvas (CPU compositor from boot) — drill skipped")
            must("context-loss drill", True, "skipped/cpu")
        else:
            lost = await page.evaluate("""() => {
              const gl2 = document.getElementById('gl').getContext('webgl2');
              const ext = gl2 && gl2.getExtension('WEBGL_lose_context');
              if (!ext) return false;
              window.__loseExt = ext;
              ext.loseContext();
              return true;
            }""")
            if not lost:
                must("context-loss drill", False, "no lose_context ext")
            else:
                # force frames so the dead context is actually exercised
                await page.evaluate("window.__calliope.gpuForceLive()")
                recovered = None
                for _ in range(40):  # up to 20 s
                    await page.evaluate("""() => {
                      const v = window.__calliope.view;
                      v.centerOn ? v.centerOn(320 + Math.random(), 256 + Math.random(), v.scale)
                                 : null;
                    }""")
                    await asyncio.sleep(0.5)
                    st = await page.evaluate("""() => ({
                      backend: window.__calliope.gpuBackend(),
                      recoveryLogged: false,
                    })""")
                    logged = any("GPU recovery" in t or "CPU compositor in charge" in t for t in console)
                    if logged:
                        recovered = st["backend"]
                        break
                # whatever happened, the map must still draw on damage
                d0 = await page.evaluate("window.__calliope.draws || 0")
                await page.evaluate("window.__calliope.view.centerOn(300, 250, 2)")
                await asyncio.sleep(1.0)
                d1 = await page.evaluate("window.__calliope.draws || 0")
                alive = d1 > d0
                detected = recovered is not None
                info.append(
                    f"GL drill: pre {pre['backend']} · detected={detected} · post backend "
                    f"{recovered or 'undetected'} · draws {d0}→{d1}"
                )
                must("context-loss drill: loss detected", detected, recovered or "silent")
                must("context-loss drill: map still draws", alive, f"{d1 - d0} draws")

        await browser.close()


def write_report(err=None):
    lines = [
        "=" * 72,
        " CALLIOPE DIAGNOSTIC · BROWSER               headless chromium · :8080",
        "=" * 72,
    ]
    lines += [" " + i for i in info]
    lines += ["", "---- checks " + "-" * 58]
    lines += rows
    if err:
        lines.append(f"[FAIL] browser probe aborted: {err}")
    n = {"PASS": 0, "WARN": 0, "FAIL": 0}
    for r in rows:
        for k in n:
            if r.startswith(f"[{k}]"):
                n[k] += 1
    lines.append(f"CHECKS: {n['PASS']} pass · {n['WARN']} warn · {n['FAIL']} fail")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(lines) + "\n")
    print("\n".join(lines))


try:
    asyncio.run(main())
    write_report()
except Exception as e:  # a crashed probe is a FAIL row, not a silent absence
    write_report(err=e)
    sys.exit(1)
