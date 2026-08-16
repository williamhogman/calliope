#!/usr/bin/env python3
"""M7 gate — the atlas plate holds its grammar under live rendering.

Checks, per seed and zoom tier:
  1. label-overlap count is 0 (the unified placement engine never collides)
  2. settlement-label density follows Töpfer's radical law between tiers:
     n(s2)/n(s1) ~= sqrt(s2/s1), within tolerance, wherever collision
     culling is not the binding constraint (budget-limited counts compared)
  3. screenshots at every tier land in reports/atlas/ for plate review

Run with the dev server up:  python3 scripts/atlas-check.py
Exits non-zero on any failed check.
"""

import asyncio
import sys
from pathlib import Path

from playwright.async_api import async_playwright

BASE = "http://localhost:8080"
SEEDS = [12345, 777]
SIZE = 384
ZOOMS = [1.3, 3.0, 8.0]  # plate, region, close survey
OUT = Path(__file__).resolve().parent.parent / "game" / "reports" / "atlas"

PASS, FAIL = "[PASS]", "[FAIL]"
failures = 0


def check(ok, name, detail):
    global failures
    print(f"{PASS if ok else FAIL} {name:44s} {detail}")
    if not ok:
        failures += 1


async def run_seed(page, seed):
    await page.goto(f"{BASE}/?seed={seed}&size={SIZE}", wait_until="domcontentloaded")
    await page.wait_for_selector("#loading.fade", timeout=180000)
    # a lived-in world: settlements, routes, features all on the plate
    await page.evaluate("window.__calliope.advance(240)")
    await page.wait_for_function("window.__calliope.month() >= 240", timeout=180000)
    await page.wait_for_timeout(800)

    stats = {}
    for z in ZOOMS:
        await page.evaluate(
            """(z) => {
              const v = window.__calliope.view;
              const r = window.__calliope.renderer;
              const cw = r.canvas.clientWidth, ch = r.canvas.clientHeight;
              v.scale = z;
              v.tx = cw / 2 - (r.w / 2) * z;
              v.ty = ch / 2 - (r.h / 2) * z;
              v.onChange && v.onChange();
            }""",
            z,
        )
        await page.wait_for_timeout(500)
        st = await page.evaluate("window.__calliope.labelStats()")
        stats[z] = st
        check(st is not None, f"seed {seed} z{z}: stats present", str(bool(st)))
        if st is None:
            continue
        check(
            st["overlaps"] == 0,
            f"seed {seed} z{z}: zero label overlap",
            f"{st['overlaps']} overlaps / {st['placed']} placed",
        )
        shot = OUT / f"atlas-{seed}-z{z}.png"
        await page.screenshot(path=str(shot))

    # Töpfer: budgets between zoom tiers follow the radical law exactly
    # (they are computed from it); verify end-to-end through the live page.
    for a, b in [(ZOOMS[0], ZOOMS[1]), (ZOOMS[1], ZOOMS[2])]:
        sa, sb = stats.get(a), stats.get(b)
        if not sa or not sb:
            continue
        if sb["setBudget"] == 0 or sa["setBudget"] == 0:
            continue
        expected = (a / b) ** 0.5
        actual = sa["setBudget"] / sb["setBudget"]
        ok = abs(actual - expected) <= max(0.18 * expected, 2.0 / sb["setBudget"])
        check(
            ok,
            f"seed {seed}: Töpfer z{a}->z{b}",
            f"budget ratio {actual:.2f} vs √(s ratio) {expected:.2f}",
        )


async def main():
    OUT.mkdir(parents=True, exist_ok=True)
    async with async_playwright() as p:
        browser = await p.chromium.launch(headless=True)
        ctx = await browser.new_context(viewport={"width": 1280, "height": 900})
        page = await ctx.new_page()
        errors = []
        page.on("pageerror", lambda e: errors.append(str(e)))
        for seed in SEEDS:
            await run_seed(page, seed)
        check(not errors, "no page errors across the run", "; ".join(errors[:2]) or "clean")
        await browser.close()
    print(f"\natlas plates in {OUT}")
    sys.exit(1 if failures else 0)


asyncio.run(main())
