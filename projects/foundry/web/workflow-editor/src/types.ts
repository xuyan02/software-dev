/** Wire types shared with the control plane's workflow API. */

export type RoleKind = 'human' | 'agent' | 'any';

export type TaskState = 'pending' | 'in_progress' | 'completed';

export type VarType = 'bool' | 'string' | 'int' | 'actor_set';

export type EventKind =
  | 'task.created'
  | 'task.state_changed'
  | 'child.created'
  | 'child.state_changed';

export interface RoleSpec {
  name: string;
  kind: RoleKind;
  min_actors: number;
  max_actors?: number | null;
}

export type InitialValue = boolean | number | string;

export interface VarSpec {
  type: VarType;
  display_name: string;
  /** The value the variable starts with. `actor_set` variables start empty. */
  initial?: InitialValue;
}

export interface OperationSpec {
  name: string;
  display_name: string;
  description: string;
  roles: string[];
  action: string;
}

export interface EventSpec {
  event: EventKind;
  action: string;
}

export interface CreateChild {
  workflow: string;
  title: string;
}

export type CreatedEffect =
  | { set_state: TaskState }
  | { create_child: CreateChild };

export interface TransitionSpec {
  on: string;
  /** Runs after `on` matched and before entering `to`. */
  action: string;
  /** A declared stage name, or `ended`. */
  to: string;
}

export interface WorkflowStage {
  name: string;
  active_roles: string[];
  vars: Record<string, VarSpec>;
  operations: OperationSpec[];
  events: EventSpec[];
  created: CreatedEffect[];
  transitions: TransitionSpec[];
}

export interface WorkflowBody {
  schema_version: number;
  roles: RoleSpec[];
  /** Task variables. They survive stage changes. */
  vars: Record<string, VarSpec>;
  initial_stage: string | null;
  stages: WorkflowStage[];
}

export type WorkflowOwner =
  | { kind: 'platform' }
  | { kind: 'account'; account_id: string; display_name: string };

export interface WorkflowDefinition {
  id: string;
  title: string;
  owner: WorkflowOwner;
  latest_version: number | null;
  draft_revision: number | null;
  editable: boolean;
  created_at: string;
  updated_at: string;
}

export interface WorkflowVersionInfo {
  version: number;
  published_by_account_id: string | null;
  published_by_display_name: string;
  published_at: string;
}

export interface WorkflowVersion {
  workflow_id: string;
  version: number;
  body: WorkflowBody;
  published_by_account_id: string | null;
  published_by_display_name: string;
  published_at: string;
}

export interface WorkflowDraftView {
  workflow_id: string;
  revision: number;
  body: WorkflowBody;
  updated_by_account_id: string;
  updated_at: string;
}

export interface StagePosition {
  x: number;
  y: number;
}

export interface WorkflowLayout {
  positions: Record<string, StagePosition>;
}

export interface ValidationIssue {
  path: string;
  code: string;
  message: string;
}

export interface Validation {
  errors: ValidationIssue[];
  warnings: ValidationIssue[];
}

export interface DraftSaved {
  revision: number;
  updated_at: string;
}

export interface WorkflowPublished {
  version: number;
  revision: number;
  warnings: ValidationIssue[];
}

export function emptyBody(): WorkflowBody {
  return {
    schema_version: 1,
    roles: [],
    vars: {},
    initial_stage: null,
    stages: [],
  };
}

export function emptyStage(name: string): WorkflowStage {
  return {
    name,
    active_roles: [],
    vars: {},
    operations: [],
    events: [],
    created: [],
    transitions: [],
  };
}
