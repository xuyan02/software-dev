import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DraftSaver } from './autosave';
import { emptyBody } from './types';

function response(status: number, payload: unknown): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

function requestBody(call: unknown[]): {
  body: unknown;
  client_save_id: string;
} {
  return JSON.parse(String((call[1] as RequestInit).body));
}

describe('DraftSaver', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('debounces, sends If-Match, and adopts the returned revision', async () => {
    const fetch = vi.fn(async (_url: string, _init?: RequestInit) =>
      response(200, { revision: 2, updated_at: '2026-11-04T10:00:00Z' }),
    );
    vi.stubGlobal('fetch', fetch);
    const body = emptyBody();
    const saved: number[] = [];
    const saver = new DraftSaver('wf', 1, () => body, {
      onState: () => {},
      onSaved: (revision) => saved.push(revision),
      onConflict: () => {},
    });

    saver.markDirty(body);
    expect(fetch).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(600);
    expect(fetch).toHaveBeenCalledTimes(1);
    const request = fetch.mock.calls[0];
    expect(String(request[0])).toContain('/api/v1/workflows/wf/draft');
    expect((request[1] as RequestInit).method).toBe('PUT');
    expect(
      (request[1] as RequestInit).headers as Record<string, string>,
    ).toMatchObject({ 'If-Match': '"1"' });
    expect(requestBody(request).body).toEqual(body);
    expect(requestBody(request).client_save_id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/,
    );
    expect(saved).toEqual([2]);
    expect(saver.getRevision()).toBe(2);
    saver.dispose();
  });

  it('retries retryable failures with the same client_save_id', async () => {
    let attempts = 0;
    const ids: string[] = [];
    const fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      attempts += 1;
      ids.push(JSON.parse(String(init?.body)).client_save_id);
      if (attempts === 1) {
        return response(500, {
          error: { code: 'internal_error', message: 'boom' },
        });
      }
      return response(200, { revision: 5, updated_at: '2026-11-04T10:00:00Z' });
    });
    vi.stubGlobal('fetch', fetch);
    const body = emptyBody();
    const saver = new DraftSaver('wf', 4, () => body, {
      onState: () => {},
      onSaved: () => {},
      onConflict: () => {},
    });

    saver.markDirty(body);
    await vi.advanceTimersByTimeAsync(600);
    expect(attempts).toBe(1);
    await vi.advanceTimersByTimeAsync(1000);
    expect(attempts).toBe(2);
    expect(ids[0]).toBe(ids[1]);
    expect(saver.getRevision()).toBe(5);
    saver.dispose();
  });

  it('stops autosave on a conflict and only overwrite resumes it', async () => {
    const revisions: Array<string | undefined> = [];
    const fetch = vi.fn(async (url: string, init?: RequestInit) => {
      if (String(url).endsWith('/draft') && init?.method === 'PUT') {
        revisions.push(
          (init.headers as Record<string, string>)['If-Match'],
        );
        if (revisions.length === 1) {
          return response(409, {
            error: {
              code: 'draft_conflict',
              message: 'conflict',
              current_revision: 7,
              current_updated_at: '2026-11-04T10:00:00Z',
            },
          });
        }
      }
      return response(200, { revision: 8, updated_at: '2026-11-04T10:00:00Z' });
    });
    vi.stubGlobal('fetch', fetch);
    const body = emptyBody();
    const conflicts: Array<number | null> = [];
    const saver = new DraftSaver('wf', 6, () => body, {
      onState: () => {},
      onSaved: () => {},
      onConflict: (revision) => conflicts.push(revision),
    });

    saver.markDirty(body);
    await vi.advanceTimersByTimeAsync(600);
    expect(conflicts).toEqual([7]);
    expect(saver.getRevision()).toBe(6);

    saver.markDirty(body);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(fetch).toHaveBeenCalledTimes(1);

    saver.overwrite(7);
    await vi.advanceTimersByTimeAsync(0);
    expect(revisions).toEqual(['"6"', '"7"']);
    expect(saver.getRevision()).toBe(8);
    saver.dispose();
  });

  it('flush saves pending edits immediately', async () => {
    const fetch = vi.fn(async (_url: string, _init?: RequestInit) =>
      response(200, { revision: 3, updated_at: '2026-11-04T10:00:00Z' }),
    );
    vi.stubGlobal('fetch', fetch);
    const body = emptyBody();
    const saver = new DraftSaver('wf', 2, () => body, {
      onState: () => {},
      onSaved: () => {},
      onConflict: () => {},
    });

    saver.markDirty(body);
    await expect(saver.flush()).resolves.toBe(true);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(saver.getRevision()).toBe(3);
    saver.dispose();
  });
});
