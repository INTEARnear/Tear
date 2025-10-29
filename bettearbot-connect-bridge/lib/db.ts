import Database from 'better-sqlite3';
import { join } from 'path';

const dbPath = join(process.cwd(), 'connections.db');
const db = new Database(dbPath);

db.exec(`
  CREATE TABLE IF NOT EXISTS connections (
    telegram_user_id TEXT PRIMARY KEY,
    x_user_id TEXT,
    near_account_id TEXT
  )
`);

export function saveXConnection(telegramUserId: string, xUserId: string): void {
  const stmt = db.prepare(
    'INSERT INTO connections (telegram_user_id, x_user_id) VALUES (?, ?) ON CONFLICT(telegram_user_id) DO UPDATE SET x_user_id = ?'
  );
  stmt.run(telegramUserId, xUserId, xUserId);
}

export function getConnection(telegramUserId: string): {
  x_user_id: string | null;
  near_account_id: string | null;
} {
  const stmt = db.prepare(
    'SELECT x_user_id, near_account_id FROM connections WHERE telegram_user_id = ?'
  );
  const row = stmt.get(telegramUserId) as
    | { x_user_id: string | null; near_account_id: string | null }
    | undefined;
  return row ?? { x_user_id: null, near_account_id: null };
}

export function saveNearConnection(telegramUserId: string, nearAccountId: string): void {
  const stmt = db.prepare(
    'INSERT INTO connections (telegram_user_id, near_account_id) VALUES (?, ?) ON CONFLICT(telegram_user_id) DO UPDATE SET near_account_id = ?'
  );
  stmt.run(telegramUserId, nearAccountId, nearAccountId);
}

export function deleteXConnection(telegramUserId: string): void {
  const stmt = db.prepare('UPDATE connections SET x_user_id = NULL WHERE telegram_user_id = ?');
  stmt.run(telegramUserId);
}

export function deleteNearConnection(telegramUserId: string): void {
  const stmt = db.prepare(
    'UPDATE connections SET near_account_id = NULL WHERE telegram_user_id = ?'
  );
  stmt.run(telegramUserId);
}
