// Loads the WebAssembly engine once and exposes static board topology.
import init, * as wasm from './wasm/isle_wasm.js';

export type Engine = typeof wasm;
export type Game = wasm.Game;

/** Owner value meaning "nobody" in per-spot arrays. */
export const EMPTY = 255;
/** Flag set on a building owner when the building is a city. */
export const CITY_FLAG = 0x80;

export interface Topology {
  tilePos: Int16Array;
  vertexPos: Int16Array;
  edgeVertices: Uint8Array;
  harborEdges: Uint8Array;
}

let ready: Promise<{ engine: Engine; topology: Topology }> | undefined;

export function loadEngine() {
  ready ??= init().then(() => ({
    engine: wasm,
    topology: {
      tilePos: wasm.tilePositions(),
      vertexPos: wasm.vertexPositions(),
      edgeVertices: wasm.edgeVertices(),
      harborEdges: wasm.harborEdges(),
    },
  }));
  return ready;
}
