-- Current development schema. Run scripts/setup_db.sh on an empty database.
-- Each user is one hospital machine account. Artifact contents and datasets
-- are not stored in this database. Credentials must be encrypted by the app.
\set ON_ERROR_STOP on

BEGIN;

CREATE TYPE provider_kind AS ENUM ('github', 'gitlab');
CREATE TYPE snapshot_source AS ENUM ('github', 'gitlab', 'local', 'platform');
CREATE TYPE run_status AS ENUM ('pending', 'started', 'finished', 'cancelled', 'failed', 'skipped');
CREATE TYPE participation_status AS ENUM ('pending', 'joined', 'removed');
CREATE TYPE removal_reason AS ENUM ('voluntary', 'permission_lost', 'unauthorized_behavior');
CREATE TYPE permission_scope AS ENUM ('organization', 'project');
CREATE TYPE execution_status AS ENUM (
    'not_started', 'starting', 'running', 'completed', 'failed', 'stopped'
);

CREATE TABLE users (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    username text NOT NULL UNIQUE,
    password_hash text NOT NULL
);

-- Persist logout revocations across API restarts. Rows may be removed after expiry.
CREATE TABLE revoked_tokens (
    jti text PRIMARY KEY,
    expires_at timestamptz NOT NULL
);

CREATE TABLE notifications (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id integer NOT NULL REFERENCES users (id),
    title text NOT NULL,
    message text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    read_at timestamptz
);

CREATE TABLE organizations (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name text NOT NULL,
    owner_user_id integer NOT NULL REFERENCES users (id)
);

CREATE TABLE roles (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    org_id integer NOT NULL REFERENCES organizations (id),
    name text NOT NULL,
    UNIQUE (org_id, name),
    UNIQUE (org_id, id)
);

CREATE TABLE user_organization (
    user_id integer NOT NULL REFERENCES users (id),
    org_id integer NOT NULL REFERENCES organizations (id),
    role_id integer,
    PRIMARY KEY (user_id, org_id),
    FOREIGN KEY (org_id, role_id) REFERENCES roles (org_id, id)
);

-- Create an organization and its owner's membership in the same transaction.
-- This deferred FK also prevents deleting an owner's membership without
-- transferring ownership or deleting the organization in that transaction.
ALTER TABLE organizations ADD CONSTRAINT organizations_owner_membership_fk
    FOREIGN KEY (owner_user_id, id)
    REFERENCES user_organization (user_id, org_id)
    DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE permissions (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name text NOT NULL UNIQUE,
    scope permission_scope NOT NULL,
    UNIQUE (id, scope),
    CHECK ((name = 'edit_roles' AND scope = 'organization')
        OR (name IN ('edit_project', 'start_deployment', 'participate_in_deployment') AND scope = 'project'))
);

CREATE TABLE role_permission (
    role_id integer NOT NULL REFERENCES roles (id),
    perm_id integer NOT NULL,
    scope permission_scope NOT NULL DEFAULT 'organization' CHECK (scope = 'organization'),
    FOREIGN KEY (perm_id, scope) REFERENCES permissions (id, scope),
    PRIMARY KEY (role_id, perm_id)
);

-- Fixed permission catalog. The runtime database role should have SELECT only
-- on permissions; future catalog changes belong in this schema.
INSERT INTO permissions (name, scope) VALUES
    ('edit_roles', 'organization'),
    ('edit_project', 'project'),
    ('start_deployment', 'project'),
    ('participate_in_deployment', 'project');

CREATE TABLE provider_connections (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id integer NOT NULL REFERENCES users (id),
    provider provider_kind NOT NULL,
    external_account_id text NOT NULL,
    external_username text,
    access_token_encrypted text NOT NULL,
    refresh_token_encrypted text,
    token_expires_at timestamptz,
    granted_permissions text,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    invalidated_at timestamptz,
    UNIQUE (user_id, provider)
);

COMMENT ON TABLE provider_connections IS
    'Personal GitHub App / GitLab access-token connections. External account IDs are not unique. Local unlink affects one connection; provider revocation affects revoked credentials. Encryption keys live outside the database.';

CREATE TABLE projects (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    org_id integer NOT NULL REFERENCES organizations (id),
    created_by_user_id integer NOT NULL REFERENCES users (id),
    name text NOT NULL,
    dockerfile_path text,
    compose_file_path text,
    build_context_path text NOT NULL DEFAULT '.',
    env_file_path text,
    command_arguments jsonb,
    input_mount_destination text NOT NULL,
    output_mount_destination text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (org_id, id)
);

COMMENT ON TABLE projects IS
    'Editable defaults for future runs. Snapshot-relative paths and Docker/Compose options must be validated by the application. Updates do not silently change existing runs.';

-- Project grants are scoped to a project in the role's own organization.
CREATE TABLE role_project_permission (
    org_id integer NOT NULL,
    role_id integer NOT NULL,
    project_id integer NOT NULL,
    perm_id integer NOT NULL,
    scope permission_scope NOT NULL DEFAULT 'project' CHECK (scope = 'project'),
    PRIMARY KEY (role_id, project_id, perm_id),
    FOREIGN KEY (org_id, role_id) REFERENCES roles (org_id, id),
    FOREIGN KEY (org_id, project_id) REFERENCES projects (org_id, id),
    FOREIGN KEY (perm_id, scope) REFERENCES permissions (id, scope)
);

CREATE TABLE snapshots (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    project_id integer NOT NULL REFERENCES projects (id),
    created_by_user_id integer NOT NULL REFERENCES users (id),
    source snapshot_source NOT NULL,
    source_repository_id text,
    source_repository_url text,
    source_branch text,
    source_commit_sha text,
    git_commit_sha text NOT NULL CHECK (git_commit_sha ~ '^[0-9a-f]{40}$'),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (project_id, id)
);

COMMENT ON TABLE snapshots IS
    'Immutable Git commits retained at refs/snapshots/<id> in storage/git/projects/<project_id>.git. git_commit_sha identifies stored files; source_commit_sha is optional external provenance. Never replace a published reference.';

CREATE TABLE deployment_runs (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    org_id integer NOT NULL,
    project_id integer NOT NULL,
    snapshot_id integer NOT NULL,
    name text NOT NULL DEFAULT 'Deployment',
    change_summary text NOT NULL DEFAULT '',
    supersedes_run_id integer UNIQUE,
    created_by_user_id integer NOT NULL REFERENCES users (id),
    started_by_user_id integer REFERENCES users (id),
    cancelled_by_user_id integer REFERENCES users (id),
    status run_status NOT NULL DEFAULT 'pending',
    terminal_reason text,
    dockerfile_path text,
    compose_file_path text,
    build_context_path text NOT NULL,
    env_file_path text,
    command_arguments jsonb,
    input_mount_destination text NOT NULL,
    output_mount_destination text NOT NULL,
    coordinator_session_id text UNIQUE,
    coordinator_endpoint text,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at timestamptz,
    ended_at timestamptz,
    UNIQUE (project_id, id),
    UNIQUE (id, snapshot_id),
    FOREIGN KEY (org_id, project_id) REFERENCES projects (org_id, id),
    FOREIGN KEY (project_id, snapshot_id) REFERENCES snapshots (project_id, id),
    FOREIGN KEY (project_id, supersedes_run_id) REFERENCES deployment_runs (project_id, id),
    CONSTRAINT run_not_own_replacement CHECK (supersedes_run_id IS DISTINCT FROM id),
    CONSTRAINT run_start_metadata CHECK ((started_at IS NULL) = (started_by_user_id IS NULL)),
    CONSTRAINT run_execution_has_start CHECK (status NOT IN ('started', 'finished', 'failed') OR started_at IS NOT NULL),
    CONSTRAINT run_unstarted_states CHECK (status NOT IN ('pending', 'skipped') OR started_at IS NULL),
    CONSTRAINT run_end_metadata CHECK (
        (status IN ('finished', 'cancelled', 'failed', 'skipped')) = (ended_at IS NOT NULL)
    ),
    CONSTRAINT run_end_after_start CHECK (ended_at IS NULL OR ended_at >= COALESCE(started_at, created_at)),
    CONSTRAINT run_start_after_creation CHECK (started_at IS NULL OR started_at >= created_at),
    CONSTRAINT run_failure_reason CHECK (status <> 'failed' OR NULLIF(trim(terminal_reason), '') IS NOT NULL),
    CONSTRAINT run_cancellation_metadata CHECK (cancelled_by_user_id IS NULL OR status = 'cancelled')
);

COMMENT ON TABLE deployment_runs IS
    'Pending publication captures a snapshot. Explicit updates create a replacement and skip the old deployment; joined users reaccept and are notified. Start requires the starter plus another joined participant. Losing one participant does not fail execution; losing all cancels it. Terminal states cannot restart.';

CREATE TABLE run_participants (
    run_id integer NOT NULL REFERENCES deployment_runs (id),
    user_id integer NOT NULL REFERENCES users (id),
    participation_status participation_status NOT NULL DEFAULT 'pending',
    accepted_snapshot_id integer,
    accepted_at timestamptz,
    local_input_path text,
    local_output_path text,
    execution_status execution_status NOT NULL DEFAULT 'not_started',
    joined_at timestamptz,
    removed_at timestamptz,
    removal_reason removal_reason,
    stop_requested_at timestamptz,
    stopped_at timestamptz,
    PRIMARY KEY (run_id, user_id),
    FOREIGN KEY (run_id, accepted_snapshot_id) REFERENCES deployment_runs (id, snapshot_id),
    CONSTRAINT participant_acceptance_pair CHECK ((accepted_snapshot_id IS NULL) = (accepted_at IS NULL)),
    CONSTRAINT participant_joined_metadata CHECK (participation_status <> 'joined' OR (
        accepted_at IS NOT NULL AND joined_at IS NOT NULL
        AND NULLIF(trim(local_input_path), '') IS NOT NULL AND NULLIF(trim(local_output_path), '') IS NOT NULL
    )),
    CONSTRAINT participant_pending_metadata CHECK (participation_status <> 'pending' OR (
        accepted_at IS NULL AND joined_at IS NULL AND execution_status = 'not_started'
    )),
    CONSTRAINT participant_removal_metadata CHECK (
        (participation_status = 'removed') = (removed_at IS NOT NULL AND removal_reason IS NOT NULL)
        AND ((removed_at IS NULL) = (removal_reason IS NULL))
    )
);

COMMENT ON TABLE run_participants IS
    'Pending means eligible and not accepted. Joined accepts this deployment snapshot/settings. Removed retains a voluntary/system reason; eligible users can rejoin only while pending. Local path changes do not invalidate acceptance. System removal and voluntary departure preserve history and request local stop; acknowledgment is separate.';

-- PostgreSQL does not automatically index the referencing side of foreign keys.
CREATE INDEX notifications_user_created_idx ON notifications (user_id, created_at DESC);
CREATE INDEX organizations_owner_idx ON organizations (owner_user_id);
CREATE INDEX memberships_org_role_idx ON user_organization (org_id, role_id);
CREATE INDEX role_permission_permission_idx ON role_permission (perm_id);
CREATE INDEX project_grants_project_idx ON role_project_permission (project_id, role_id);
CREATE INDEX projects_creator_idx ON projects (created_by_user_id);
CREATE INDEX snapshots_creator_idx ON snapshots (created_by_user_id);
CREATE INDEX runs_project_snapshot_idx ON deployment_runs (project_id, snapshot_id);
CREATE INDEX runs_org_status_idx ON deployment_runs (org_id, status);
CREATE INDEX runs_creator_idx ON deployment_runs (created_by_user_id);
CREATE INDEX runs_starter_idx ON deployment_runs (started_by_user_id);
CREATE INDEX participants_user_idx ON run_participants (user_id);

-- Deployment functions and triggers are kept together in one rules file.
\ir deployment_rules.sql

-- Runtime services must authenticate actors, validate file paths, safely store
-- snapshots before publication, and authorize system removal. SQL actor IDs must
-- come from verified sessions, never client-supplied identity. Stop requests need
-- coordinator/local-agent handling; unauthorized-behavior detection is future work.
-- Restrict the runtime role's direct writes to lifecycle tables and snapshot files.
COMMIT;
