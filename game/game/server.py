"""Calliope server: world generation + simulation API and the map client."""

import json
import struct
import threading
from collections import OrderedDict
from pathlib import Path

import numpy as np
from fastapi import FastAPI, HTTPException
from fastapi.middleware.gzip import GZipMiddleware
from fastapi.responses import Response
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel

from game.world import World

app = FastAPI(title="Calliope")
app.add_middleware(GZipMiddleware, minimum_size=4096)

_WORLDS: "OrderedDict[str, World]" = OrderedDict()
_LOCK = threading.Lock()
_MAX_WORLDS = 6


class GenerateRequest(BaseModel):
    seed: int | None = None
    size: int = 512


class TickRequest(BaseModel):
    months: int = 1


def _world_id(seed: int, size: int) -> str:
    return f"{seed}-{size}"


def _pack(header: dict, arrays: list[tuple[str, np.ndarray]]) -> bytes:
    """[u32 header_len][header json (padded to 4)][raw little-endian arrays]"""
    entries = []
    blobs = []
    offset = 0
    for name, arr in arrays:
        arr = np.ascontiguousarray(arr)
        raw = arr.tobytes()
        entries.append({
            "name": name,
            "dtype": str(arr.dtype),
            "shape": list(arr.shape),
            "offset": offset,
            "nbytes": len(raw),
        })
        blobs.append(raw)
        offset += len(raw)
    header = dict(header)
    header["arrays"] = entries
    hjson = json.dumps(header).encode("utf-8")
    pad = (-len(hjson)) % 4
    hjson += b" " * pad
    return struct.pack("<I", len(hjson)) + hjson + b"".join(blobs)


def _get_world(world_id: str) -> World:
    with _LOCK:
        w = _WORLDS.get(world_id)
    if w is None:
        raise HTTPException(404, "world not found — generate it first")
    return w


@app.get("/api/health")
def health():
    return {"ok": True}


@app.post("/api/world")
def generate(req: GenerateRequest):
    seed = req.seed if req.seed is not None else int(np.random.default_rng().integers(1, 2**31))
    size = int(req.size)
    if size not in (256, 384, 512):
        raise HTTPException(400, "size must be 256, 384 or 512")
    wid = _world_id(seed, size)
    with _LOCK:
        w = _WORLDS.get(wid)
        if w is not None:
            _WORLDS.move_to_end(wid)
    if w is None:
        w = World(seed, size)
        with _LOCK:
            _WORLDS[wid] = w
            _WORLDS.move_to_end(wid)
            while len(_WORLDS) > _MAX_WORLDS:
                _WORLDS.popitem(last=False)

    header = {"id": wid, **w.meta()}
    payload = _pack(header, [
        ("height", w.height),
        ("tmean", w.tmean),
        ("tamp", w.tamp),
        ("precip", w.precip),
        ("discharge", w.discharge),
        ("fertility", w.fertility),
        ("biomes", w.biomes),
        ("flags", w.flags()),
    ])
    return Response(payload, media_type="application/octet-stream")


@app.post("/api/world/{world_id}/tick")
def tick(world_id: str, req: TickRequest):
    w = _get_world(world_id)
    with _LOCK:
        res = w.tick(req.months)
    out = {
        "month": w.month,
        "settlements": w.settlements,
        "events": res["events"],
    }
    if res["founded"]:
        out["routes"] = w.routes
    return out


_WEB = Path(__file__).resolve().parent.parent / "web"
app.mount("/", StaticFiles(directory=str(_WEB), html=True), name="web")
