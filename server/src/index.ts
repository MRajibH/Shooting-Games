import { createReadStream, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WebSocketServer } from 'ws';
import { DEFAULT_PORT, decodeClient } from '@riftline/shared';
import type { Game } from './game';
import { Rooms } from './rooms';

const PORT = Number(process.env.PORT) || DEFAULT_PORT;
const STATIC = fileURLToPath(new URL('../../client/dist', import.meta.url));
const MIME: Record<string, string> = {
  '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css',
  '.svg': 'image/svg+xml', '.png': 'image/png', '.json': 'application/json',
};

// Serves the built client (npm run build). In dev, Vite serves it and proxies /ws here.
const http = createServer((req, res) => {
  const path = normalize(decodeURIComponent((req.url ?? '/').split('?')[0]!)).replace(/^(\.\.[/\\])+/, '');
  let file = join(STATIC, path);
  if (!file.startsWith(STATIC) || !existsSync(file) || statSync(file).isDirectory()) file = join(STATIC, 'index.html');
  if (!existsSync(file)) { res.writeHead(404).end('Client not built. Run: npm run build'); return; }
  res.writeHead(200, { 'Content-Type': MIME[extname(file)] ?? 'application/octet-stream' });
  createReadStream(file).pipe(res);
});

const rooms = new Rooms();
const wss = new WebSocketServer({ server: http, path: '/ws', maxPayload: 16 * 1024 });
let nextId = 1;

wss.on('connection', (ws) => {
  const id = nextId++;
  let game: Game | null = null;

  ws.on('message', (data) => {
    let msg;
    try { msg = decodeClient(data.toString()); } catch { return; }
    if (msg.t === 'join' && !game) {
      const name = String(msg.name ?? '').replace(/[^\w -]/g, '').slice(0, 16) || `Pilot-${id}`;
      game = rooms.find(typeof msg.room === 'string' ? msg.room : undefined);
      game.add({ id, name, send: (d) => ws.readyState === ws.OPEN && ws.send(d) });
    } else if (msg.t === 'input' && game && Array.isArray(msg.frames)) {
      game.pushInputs(id, msg.frames.slice(0, 8));
    }
  });

  ws.on('close', () => { game?.remove(id); rooms.prune(); });
});

http.listen(PORT, () => console.log(`RIFTLINE server on http://localhost:${PORT}`));
