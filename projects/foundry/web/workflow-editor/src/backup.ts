import type { WorkflowBody } from './types';

/**
 * A local crash backup of unsaved edits. It is only a per-browser recovery
 * aid; the server draft is authoritative.
 */
export interface DraftBackup {
  revision: number;
  body: WorkflowBody;
  client_save_id: string;
  savedAt: string;
}

function key(workflow: string): string {
  return `foundry.workflow-draft.${workflow}`;
}

export function readBackup(workflow: string): DraftBackup | null {
  try {
    const raw = localStorage.getItem(key(workflow));
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<DraftBackup>;
    if (
      typeof parsed.revision !== 'number' ||
      typeof parsed.body !== 'object' ||
      parsed.body === null
    ) {
      return null;
    }
    return parsed as DraftBackup;
  } catch {
    return null;
  }
}

export function writeBackup(
  workflow: string,
  revision: number,
  body: WorkflowBody,
  clientSaveId: string,
): void {
  try {
    const backup: DraftBackup = {
      revision,
      body,
      client_save_id: clientSaveId,
      savedAt: new Date().toISOString(),
    };
    localStorage.setItem(key(workflow), JSON.stringify(backup));
  } catch {
    // Private mode, quota, or disabled storage: editing must keep working.
  }
}

export function clearBackup(workflow: string): void {
  try {
    localStorage.removeItem(key(workflow));
  } catch {
    // Ignore.
  }
}
