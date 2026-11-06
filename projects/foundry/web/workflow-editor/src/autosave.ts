import { ApiError, api } from './api';
import { clearBackup, writeBackup } from './backup';
import type { WorkflowBody } from './types';

export type SaveState =
  | 'saved'
  | 'dirty'
  | 'saving'
  | 'retrying'
  | 'failed'
  | 'conflict';

interface Snapshot {
  body: WorkflowBody;
  saveId: string;
}

export interface DraftSaverCallbacks {
  onState(state: SaveState): void;
  onSaved(revision: number, body: WorkflowBody): void;
  onConflict(currentRevision: number | null): void;
  onFailure?(error: unknown): void;
}

const DEBOUNCE_MS = 600;
const MAX_WAIT_MS = 5000;
const MAX_RETRY_MS = 30000;

function clone(body: WorkflowBody): WorkflowBody {
  return structuredClone(body);
}

/**
 * Debounced, single-flight draft autosave.
 *
 * The saver owns the authoritative revision: every successful save advances it
 * from the server response, and every retry reuses the snapshot it was born
 * from. New edits always produce a new `client_save_id`.
 */
export class DraftSaver {
  private revision: number;
  private pending: Snapshot | null = null;
  private inFlight = false;
  private state: SaveState = 'saved';
  private debounceTimer: ReturnType<typeof setTimeout> | null = null;
  private maxWaitTimer: ReturnType<typeof setTimeout> | null = null;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private retryDelay = 1000;
  private waiters: Array<(ok: boolean) => void> = [];
  private epoch = 0;
  private disposed = false;

  constructor(
    private readonly workflow: string,
    revision: number,
    private readonly getBody: () => WorkflowBody,
    private readonly callbacks: DraftSaverCallbacks,
  ) {
    this.revision = revision;
  }

  getRevision(): number {
    return this.revision;
  }

  hasUnsaved(): boolean {
    return (
      this.pending !== null ||
      this.state === 'dirty' ||
      this.state === 'saving' ||
      this.state === 'retrying' ||
      this.state === 'failed'
    );
  }

  /** Records a local edit and schedules the debounced save. */
  markDirty(body: WorkflowBody): void {
    if (this.disposed) return;
    if (this.state === 'conflict') {
      writeBackup(this.workflow, this.revision, body, crypto.randomUUID());
      return;
    }
    const saveId = crypto.randomUUID();
    writeBackup(this.workflow, this.revision, body, saveId);
    this.pending = { body: clone(body), saveId };
    this.setState('dirty');
    this.schedule(DEBOUNCE_MS);
    if (this.maxWaitTimer === null) {
      this.maxWaitTimer = setTimeout(() => this.save(), MAX_WAIT_MS);
    }
  }

  /** Saves immediately and resolves once the pending edits are durable. */
  flush(): Promise<boolean> {
    if (this.state === 'conflict') return Promise.resolve(false);
    if (!this.pending && !this.inFlight) return Promise.resolve(true);
    return new Promise((resolve) => {
      this.waiters.push(resolve);
      this.clearTimers();
      this.save();
    });
  }

  /**
   * Explicitly overwrites the server draft with the current body after a
   * conflict, using the revision the server reported.
   */
  overwrite(currentRevision: number): void {
    this.epoch += 1;
    this.revision = currentRevision;
    this.retryDelay = 1000;
    this.pending = { body: clone(this.getBody()), saveId: crypto.randomUUID() };
    this.writePendingBackup();
    this.setState('dirty');
    this.save();
  }

  /** Adopts a server draft after the user chose to discard local edits. */
  reset(revision: number): void {
    this.epoch += 1;
    this.revision = revision;
    this.pending = null;
    this.retryDelay = 1000;
    this.clearTimers();
    clearBackup(this.workflow);
    this.setState('saved');
    this.resolveWaiters(false);
  }

  /** Last-chance save on page hide; failures are irrelevant by then. */
  saveKeepalive(): void {
    if (this.disposed || this.state === 'conflict') return;
    if (!this.hasUnsaved()) return;
    const body = this.pending?.body ?? this.getBody();
    const saveId = this.pending?.saveId ?? crypto.randomUUID();
    writeBackup(this.workflow, this.revision, body, saveId);
    void api
      .saveDraft(this.workflow, body, saveId, this.revision, true)
      .catch(() => {
        // The localStorage backup covers a failed keepalive.
      });
  }

  dispose(): void {
    this.disposed = true;
    this.clearTimers();
    this.resolveWaiters(false);
  }

  private schedule(ms: number): void {
    if (this.debounceTimer !== null) clearTimeout(this.debounceTimer);
    this.debounceTimer = setTimeout(() => this.save(), ms);
  }

  private save(): void {
    this.clearTimers();
    if (
      this.disposed ||
      this.state === 'conflict' ||
      this.inFlight ||
      !this.pending
    ) {
      return;
    }
    const snapshot = this.pending;
    this.pending = null;
    this.inFlight = true;
    this.setState('saving');
    const epoch = this.epoch;
    api
      .saveDraft(
        this.workflow,
        snapshot.body,
        snapshot.saveId,
        this.revision,
      )
      .then((saved) => {
        if (epoch !== this.epoch || this.disposed) return;
        this.revision = saved.revision;
        this.retryDelay = 1000;
        this.callbacks.onSaved(saved.revision, snapshot.body);
        if (this.pending) {
          this.setState('dirty');
          this.schedule(0);
        } else {
          clearBackup(this.workflow);
          this.setState('saved');
          this.resolveWaiters(true);
        }
      })
      .catch((error: unknown) => {
        if (epoch !== this.epoch || this.disposed) return;
        this.fail(snapshot, error);
      })
      .finally(() => {
        this.inFlight = false;
        if (this.pending && this.state === 'dirty') this.schedule(0);
      });
  }

  private fail(snapshot: Snapshot, error: unknown): void {
    this.callbacks.onFailure?.(error);
    if (error instanceof ApiError && error.status === 409) {
      this.pending = snapshot;
      this.setState('conflict');
      const current = error.details.current_revision ?? null;
      this.callbacks.onConflict(current);
      this.resolveWaiters(false);
      return;
    }
    const retryable = !(error instanceof ApiError) || error.status >= 500;
    this.pending ??= snapshot;
    if (!retryable) {
      this.setState('failed');
      this.resolveWaiters(false);
      return;
    }
    this.setState('retrying');
    this.retryTimer = setTimeout(() => this.save(), this.retryDelay);
    this.retryDelay = Math.min(this.retryDelay * 2, MAX_RETRY_MS);
  }

  private writePendingBackup(): void {
    if (!this.pending) return;
    writeBackup(
      this.workflow,
      this.revision,
      this.pending.body,
      this.pending.saveId,
    );
  }

  private setState(state: SaveState): void {
    if (this.state === state) return;
    this.state = state;
    this.callbacks.onState(state);
  }

  private resolveWaiters(ok: boolean): void {
    const waiters = this.waiters;
    this.waiters = [];
    for (const resolve of waiters) resolve(ok);
  }

  private clearTimers(): void {
    if (this.debounceTimer !== null) clearTimeout(this.debounceTimer);
    if (this.maxWaitTimer !== null) clearTimeout(this.maxWaitTimer);
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
    this.debounceTimer = null;
    this.maxWaitTimer = null;
    this.retryTimer = null;
  }
}
