import type {
  DraftSaved,
  Validation,
  WorkflowBody,
  WorkflowDefinition,
  WorkflowDraftView,
  WorkflowLayout,
  WorkflowPublished,
  WorkflowVersion,
  WorkflowVersionInfo,
} from './types';

const PREFIX = '/api/v1/workflows';

interface ErrorEnvelope {
  code?: string;
  message?: string;
  errors?: Validation['errors'];
  warnings?: Validation['warnings'];
  current_revision?: number;
  current_updated_at?: string;
}

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    readonly details: ErrorEnvelope,
  ) {
    super(message);
  }

  get validation(): Validation | null {
    if (this.code !== 'workflow_invalid' || !this.details.errors) return null;
    return {
      errors: this.details.errors,
      warnings: this.details.warnings ?? [],
    };
  }
}

interface RequestOptions {
  body?: unknown;
  ifMatch?: number;
  keepalive?: boolean;
}

async function request<T>(
  method: string,
  path: string,
  options: RequestOptions = {},
): Promise<T> {
  const headers: Record<string, string> = {};
  if (options.body !== undefined) headers['Content-Type'] = 'application/json';
  if (options.ifMatch !== undefined) headers['If-Match'] = `"${options.ifMatch}"`;

  const response = await fetch(`${PREFIX}${path}`, {
    method,
    headers,
    body: options.body === undefined ? undefined : JSON.stringify(options.body),
    credentials: 'same-origin',
    keepalive: options.keepalive,
  });
  const text = await response.text();
  let payload: unknown;
  try {
    payload = text === '' ? undefined : JSON.parse(text);
  } catch {
    payload = undefined;
  }
  if (!response.ok) {
    const envelope = (payload as { error?: ErrorEnvelope } | undefined)?.error ?? {};
    throw new ApiError(
      response.status,
      envelope.code ?? 'unknown_error',
      envelope.message ?? response.statusText,
      envelope,
    );
  }
  return payload as T;
}

function id(value: string): string {
  return encodeURIComponent(value);
}

export const api = {
  list() {
    return request<WorkflowDefinition[]>('GET', '');
  },
  definition(workflow: string) {
    return request<WorkflowDefinition>('GET', `/${id(workflow)}`);
  },
  versions(workflow: string) {
    return request<WorkflowVersionInfo[]>('GET', `/${id(workflow)}/versions`);
  },
  version(workflow: string, version: number) {
    return request<WorkflowVersion>(
      'GET',
      `/${id(workflow)}/versions/${version}`,
    );
  },
  openDraft(workflow: string) {
    return request<WorkflowDraftView>('POST', `/${id(workflow)}/draft`);
  },
  saveDraft(
    workflow: string,
    body: WorkflowBody,
    clientSaveId: string,
    revision: number,
    keepalive = false,
  ) {
    return request<DraftSaved>('PUT', `/${id(workflow)}/draft`, {
      body: { body, client_save_id: clientSaveId },
      ifMatch: revision,
      keepalive,
    });
  },
  validate(workflow: string, body: WorkflowBody) {
    return request<Validation>('POST', `/${id(workflow)}/validate`, {
      body: { body },
    });
  },
  publish(workflow: string, revision: number) {
    return request<WorkflowPublished>('POST', `/${id(workflow)}/publish`, {
      ifMatch: revision,
    });
  },
  layout(workflow: string) {
    return request<WorkflowLayout>('GET', `/${id(workflow)}/layout`);
  },
  saveLayout(
    workflow: string,
    layout: WorkflowLayout,
    keepalive = false,
  ) {
    return request<WorkflowLayout>('PUT', `/${id(workflow)}/layout`, {
      body: layout,
      keepalive,
    });
  },
  /** Copies a published platform version into a new account workflow. */
  copyFromPlatform(title: string, workflowId: string, version: number) {
    return request<{ definition: WorkflowDefinition; draft: WorkflowDraftView }>(
      'POST',
      '',
      { body: { title, copy_from: { workflow_id: workflowId, version } } },
    );
  },
};
