-- Initial PostgreSQL schema for a fresh database; not an upgrade migration.
-- Each user is one hospital machine account. Artifact contents and datasets
-- are not stored in this database. Credentials must be encrypted by the app.
BEGIN;

CREATE TYPE provider_kind AS ENUM ('github', 'gitlab');
CREATE TYPE snapshot_source AS ENUM ('github', 'gitlab', 'platform');
CREATE TYPE run_status AS ENUM ('pending', 'running', 'completed', 'failed', 'cancelled');
CREATE TYPE participation_status AS ENUM ('joined', 'withdrawn', 'revoked');
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
    name text NOT NULL UNIQUE
);

CREATE TABLE role_permission (
    role_id integer NOT NULL REFERENCES roles (id),
    perm_id integer NOT NULL REFERENCES permissions (id),
    PRIMARY KEY (role_id, perm_id)
);

-- Fixed permission catalog. The runtime database role should have SELECT only
-- on permissions; future catalog changes belong in migrations/code.
INSERT INTO permissions (name) VALUES
    ('edit_roles'),
    ('edit_project'),
    ('start_deployment'),
    ('participate_in_deployment');

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
    'Personal GitHub App / GitLab OAuth connections. External account IDs are not unique. Local unlink affects one connection; provider revocation affects revoked credentials. Encryption keys live outside the database.';

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

CREATE TABLE snapshots (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    project_id integer NOT NULL REFERENCES projects (id),
    created_by_user_id integer NOT NULL REFERENCES users (id),
    source snapshot_source NOT NULL,
    source_repository_id text,
    source_repository_url text,
    source_branch text,
    source_commit_sha text,
    storage_key text NOT NULL UNIQUE,
    content_sha256 text NOT NULL CHECK (content_sha256 ~ '^[0-9a-fA-F]{64}$'),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (project_id, id)
);

COMMENT ON TABLE snapshots IS
    'Immutable entire-repository files without Git history, or platform-authored code. Restrict UPDATE in the runtime role and make artifacts immutable. Survives provider unlinking. Published through pending runs to eligible members.';

CREATE TABLE deployment_runs (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    org_id integer NOT NULL,
    project_id integer NOT NULL,
    snapshot_id integer NOT NULL,
    created_by_user_id integer NOT NULL REFERENCES users (id),
    started_by_user_id integer REFERENCES users (id),
    status run_status NOT NULL DEFAULT 'pending',
    configuration_revision integer NOT NULL DEFAULT 1 CHECK (configuration_revision > 0),
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
    finished_at timestamptz,
    FOREIGN KEY (org_id, project_id) REFERENCES projects (org_id, id),
    FOREIGN KEY (project_id, snapshot_id) REFERENCES snapshots (project_id, id),
    CONSTRAINT run_start_metadata CHECK (
        (started_at IS NULL) = (started_by_user_id IS NULL)
    ),
    CONSTRAINT run_running_has_start CHECK (
        status <> 'running' OR started_at IS NOT NULL
    ),
    CONSTRAINT run_pending_has_no_start CHECK (
        status <> 'pending' OR (started_at IS NULL AND finished_at IS NULL)
    ),
    CONSTRAINT run_finish_after_start CHECK (
        finished_at IS NULL OR started_at IS NULL OR finished_at >= started_at
    )
);

COMMENT ON TABLE deployment_runs IS
    'One execution per row, using an immutable snapshot selection. While pending, shared configuration edits must atomically increment revision and clear every acceptance. Start must lock the run, freeze settings, and check current starter/participant permissions and accepted revisions. Platform hosts coordinator. New runs require fresh acceptance.';

CREATE TABLE run_participants (
    run_id integer NOT NULL REFERENCES deployment_runs (id),
    user_id integer NOT NULL REFERENCES users (id),
    participation_status participation_status NOT NULL DEFAULT 'joined',
    accepted_configuration_revision integer CHECK (accepted_configuration_revision > 0),
    accepted_at timestamptz,
    local_input_path text NOT NULL,
    local_output_path text NOT NULL,
    execution_status execution_status NOT NULL DEFAULT 'not_started',
    joined_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    withdrawn_at timestamptz,
    revoked_at timestamptz,
    stop_requested_at timestamptz,
    stopped_at timestamptz,
    PRIMARY KEY (run_id, user_id),
    CONSTRAINT participant_acceptance_pair CHECK (
        (accepted_configuration_revision IS NULL) = (accepted_at IS NULL)
    ),
    CONSTRAINT participant_withdrawal_recorded CHECK (
        participation_status <> 'withdrawn' OR withdrawn_at IS NOT NULL
    ),
    CONSTRAINT participant_revocation_recorded CHECK (
        participation_status <> 'revoked' OR revoked_at IS NOT NULL
    )
);

COMMENT ON TABLE run_participants IS
    'Acceptance covers one run and shared revision, not local paths. Mount input read-only and output writable; datasets stay local. Check current membership and participation permission in the run organization. Revocation rejects coordinator updates and requests local stop; record stop acknowledgment separately. Keep historical records after membership removal.';

-- PostgreSQL does not automatically index the referencing side of foreign keys.
CREATE INDEX notifications_user_created_idx ON notifications (user_id, created_at DESC);
CREATE INDEX organizations_owner_idx ON organizations (owner_user_id);
CREATE INDEX memberships_org_role_idx ON user_organization (org_id, role_id);
CREATE INDEX role_permission_permission_idx ON role_permission (perm_id);
CREATE INDEX projects_creator_idx ON projects (created_by_user_id);
CREATE INDEX snapshots_creator_idx ON snapshots (created_by_user_id);
CREATE INDEX runs_project_snapshot_idx ON deployment_runs (project_id, snapshot_id);
CREATE INDEX runs_org_status_idx ON deployment_runs (org_id, status);
CREATE INDEX runs_creator_idx ON deployment_runs (created_by_user_id);
CREATE INDEX runs_starter_idx ON deployment_runs (started_by_user_id);
CREATE INDEX participants_user_idx ON run_participants (user_id);

-- Application responsibilities (not inferred from a database connection):
-- * Only the current owner may transfer ownership; owners implicitly have all permissions.
-- * Role editors may grant only permissions they possess.
-- * Permission/membership loss revokes affected participation, including running jobs.
-- * Synchronize acceptance/configuration/start operations by locking the run first.
-- * Participants may withdraw while pending; local path changes preserve acceptance.
-- * Set updated_at explicitly on updates to projects and provider_connections.
-- * Use a restricted runtime DB role, separate from the schema/migration owner.
-- Foreign keys use NO ACTION intentionally to preserve history and avoid implicit
-- deletion of runs, snapshots, or participants; deletion needs an explicit policy.
COMMIT;
