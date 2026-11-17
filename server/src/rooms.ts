import { MAX_PLAYERS } from '@riftline/shared';
import { Game } from './game';

/** Quick play fills the first open room; a room code joins or creates that room. */
export class Rooms {
  private games = new Map<string, Game>();
  private next = 1;

  find(code?: string): Game {
    if (code) {
      const key = code.toUpperCase().slice(0, 8);
      let g = this.games.get(key);
      if (!g) { g = new Game(key); this.games.set(key, g); }
      if (g.size < MAX_PLAYERS) return g;
    }
    for (const g of this.games.values()) if (g.size < MAX_PLAYERS) return g;
    const g = new Game(`R${this.next++}`);
    this.games.set(g.room, g);
    return g;
  }

  prune(): void {
    for (const [k, g] of this.games) if (g.size === 0) this.games.delete(k);
  }
}
