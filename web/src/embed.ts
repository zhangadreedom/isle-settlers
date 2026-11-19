// Embedded single-player build (`pnpm build:embed`): for hosting the game as
// static files inside a sandboxed page, e.g. a claude.ai artifact. There is
// no game server, file downloads are blocked and WebGPU may be unavailable,
// so online play and replay downloads are hidden and WebGL2 is used.
export const EMBEDDED = import.meta.env.VITE_EMBED === '1';
