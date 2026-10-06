-- Included inside the schema/migration transaction. These functions run with
-- caller privileges, not SECURITY DEFINER. The API must supply verified actor IDs.

CREATE OR REPLACE FUNCTION has_project_permission(actor integer, project integer, permission_name text)
RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT EXISTS (
        SELECT 1 FROM projects p
        JOIN organizations o ON o.id = p.org_id
        JOIN user_organization m ON m.org_id = o.id AND m.user_id = actor
        WHERE p.id = project
          AND permission_name IN ('edit_project', 'start_deployment', 'participate_in_deployment')
          AND (o.owner_user_id = actor OR EXISTS (
              SELECT 1 FROM role_project_permission g JOIN permissions perm ON perm.id = g.perm_id
              WHERE g.role_id = m.role_id AND g.project_id = p.id AND perm.name = permission_name
          ))
    );
$$;

CREATE OR REPLACE FUNCTION has_organization_permission(actor integer, organization integer, permission_name text)
RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT EXISTS (
        SELECT 1 FROM organizations o
        JOIN user_organization m ON m.org_id = o.id AND m.user_id = actor
        WHERE o.id = organization AND permission_name = 'edit_roles'
          AND (o.owner_user_id = actor OR EXISTS (
              SELECT 1 FROM role_permission g JOIN permissions perm ON perm.id = g.perm_id
              WHERE g.role_id = m.role_id AND perm.name = permission_name
          ))
    );
$$;

CREATE OR REPLACE FUNCTION protect_snapshot() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'Published snapshots are immutable; capture a new snapshot';
END;
$$;
CREATE TRIGGER snapshot_immutable BEFORE UPDATE ON snapshots
FOR EACH ROW EXECUTE FUNCTION protect_snapshot();

CREATE OR REPLACE FUNCTION guard_deployment() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.status <> 'pending' THEN
            RAISE EXCEPTION 'New deployments must be pending';
        END IF;
        IF NOT has_project_permission(NEW.created_by_user_id, NEW.project_id, 'edit_project') THEN
            RAISE EXCEPTION 'Project edit permission required to publish a deployment';
        END IF;
        RETURN NEW;
    END IF;

    IF ROW(NEW.org_id, NEW.project_id, NEW.snapshot_id, NEW.supersedes_run_id,
           NEW.created_by_user_id, NEW.created_at, NEW.name, NEW.change_summary,
           NEW.dockerfile_path, NEW.compose_file_path, NEW.build_context_path,
           NEW.env_file_path, NEW.command_arguments, NEW.input_mount_destination, NEW.output_mount_destination)
       IS DISTINCT FROM
       ROW(OLD.org_id, OLD.project_id, OLD.snapshot_id, OLD.supersedes_run_id,
           OLD.created_by_user_id, OLD.created_at, OLD.name, OLD.change_summary,
           OLD.dockerfile_path, OLD.compose_file_path, OLD.build_context_path,
           OLD.env_file_path, OLD.command_arguments, OLD.input_mount_destination, OLD.output_mount_destination) THEN
        RAISE EXCEPTION 'Published files and shared settings are immutable; publish another deployment';
    END IF;
    IF OLD.status <> 'pending' AND ROW(NEW.started_at, NEW.started_by_user_id)
       IS DISTINCT FROM ROW(OLD.started_at, OLD.started_by_user_id) THEN
        RAISE EXCEPTION 'Deployment start metadata is immutable';
    END IF;
    IF OLD.status IN ('finished', 'failed', 'cancelled', 'skipped') AND NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'Terminal deployments are immutable';
    END IF;
    IF NEW.status = OLD.status THEN RETURN NEW; END IF;
    IF NOT ((OLD.status = 'pending' AND NEW.status IN ('started', 'cancelled', 'skipped'))
         OR (OLD.status = 'started' AND NEW.status IN ('finished', 'failed', 'cancelled'))) THEN
        RAISE EXCEPTION 'Invalid deployment transition: % to %', OLD.status, NEW.status;
    END IF;

    IF NEW.status = 'started' THEN
        IF NOT has_project_permission(NEW.started_by_user_id, NEW.project_id, 'start_deployment') THEN
            RAISE EXCEPTION 'Start deployment permission required';
        END IF;
        IF NOT EXISTS (SELECT 1 FROM run_participants
                       WHERE run_id = NEW.id AND user_id = NEW.started_by_user_id AND participation_status = 'joined') THEN
            RAISE EXCEPTION 'The starter must have joined';
        END IF;
        IF (SELECT count(*) FROM run_participants WHERE run_id = NEW.id AND participation_status = 'joined') < 2 THEN
            RAISE EXCEPTION 'The starter and at least one other participant must join';
        END IF;
        IF EXISTS (SELECT 1 FROM run_participants WHERE run_id = NEW.id AND participation_status = 'joined'
                   AND NOT has_project_permission(user_id, NEW.project_id, 'participate_in_deployment')) THEN
            RAISE EXCEPTION 'Joined participants must retain participation permission';
        END IF;
    END IF;
    IF NEW.status = 'cancelled' AND NEW.cancelled_by_user_id IS NOT NULL
       AND NOT has_project_permission(NEW.cancelled_by_user_id, NEW.project_id, 'start_deployment') THEN
        RAISE EXCEPTION 'Start deployment permission required to cancel';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER deployment_lifecycle BEFORE INSERT OR UPDATE ON deployment_runs
FOR EACH ROW EXECUTE FUNCTION guard_deployment();

CREATE OR REPLACE FUNCTION check_replacement() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE current_run deployment_runs; previous_run deployment_runs;
BEGIN
    -- Deferred to allow creating the replacement and skipping its predecessor atomically.
    SELECT * INTO current_run FROM deployment_runs WHERE id = NEW.id;
    IF current_run.status = 'skipped' AND NOT EXISTS (
        SELECT 1 FROM deployment_runs WHERE supersedes_run_id = current_run.id
    ) THEN RAISE EXCEPTION 'Skipped deployment requires a replacement'; END IF;
    IF current_run.supersedes_run_id IS NOT NULL THEN
        SELECT * INTO previous_run FROM deployment_runs WHERE id = current_run.supersedes_run_id;
        IF previous_run.status <> 'skipped' OR previous_run.snapshot_id = current_run.snapshot_id THEN
            RAISE EXCEPTION 'Replacement must supersede a skipped deployment with a new snapshot';
        END IF;
    END IF;
    RETURN NULL;
END;
$$;
CREATE CONSTRAINT TRIGGER deployment_replacement AFTER INSERT OR UPDATE ON deployment_runs
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION check_replacement();

CREATE OR REPLACE FUNCTION guard_participant() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE deployment deployment_runs;
BEGIN
    -- All participant and start operations serialize through the deployment row.
    SELECT * INTO deployment FROM deployment_runs WHERE id = NEW.run_id FOR UPDATE;
    IF TG_OP = 'UPDATE' THEN
        IF ROW(NEW.run_id, NEW.user_id) IS DISTINCT FROM ROW(OLD.run_id, OLD.user_id) THEN
            RAISE EXCEPTION 'Participant identity is immutable';
        END IF;
        IF deployment.status IN ('finished', 'failed', 'cancelled', 'skipped') AND NEW IS DISTINCT FROM OLD THEN
            -- A local stop acknowledgment may arrive after termination.
            IF ROW(NEW.participation_status, NEW.accepted_snapshot_id, NEW.accepted_at,
                   NEW.local_input_path, NEW.local_output_path, NEW.joined_at, NEW.removed_at, NEW.removal_reason)
               IS DISTINCT FROM ROW(OLD.participation_status, OLD.accepted_snapshot_id, OLD.accepted_at,
                   OLD.local_input_path, OLD.local_output_path, OLD.joined_at, OLD.removed_at, OLD.removal_reason) THEN
                RAISE EXCEPTION 'Historical participation is immutable';
            END IF;
            RETURN NEW;
        END IF;
    END IF;
    IF TG_OP = 'INSERT' OR NEW.participation_status IS DISTINCT FROM OLD.participation_status THEN
        IF NEW.participation_status IN ('pending', 'joined') THEN
            IF deployment.status <> 'pending' THEN RAISE EXCEPTION 'Joining is only allowed while pending'; END IF;
            IF NOT has_project_permission(NEW.user_id, deployment.project_id, 'participate_in_deployment') THEN
                RAISE EXCEPTION 'Participation permission required';
            END IF;
        END IF;
        IF NEW.participation_status = 'removed' AND deployment.status NOT IN ('pending', 'started') THEN
            RAISE EXCEPTION 'Removal is only allowed before termination';
        END IF;
    END IF;
    IF TG_OP = 'UPDATE' AND deployment.status = 'started'
       AND ROW(NEW.accepted_snapshot_id, NEW.accepted_at, NEW.joined_at)
           IS DISTINCT FROM ROW(OLD.accepted_snapshot_id, OLD.accepted_at, OLD.joined_at) THEN
        RAISE EXCEPTION 'Started deployment acceptance is immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER participant_lifecycle BEFORE INSERT OR UPDATE ON run_participants
FOR EACH ROW EXECUTE FUNCTION guard_participant();

CREATE OR REPLACE FUNCTION cancel_empty_deployment() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.participation_status = 'removed' THEN
        UPDATE deployment_runs SET status = 'cancelled', ended_at = clock_timestamp(),
            terminal_reason = 'No participants remaining'
        WHERE id = NEW.run_id AND status = 'started'
          AND NOT EXISTS (SELECT 1 FROM run_participants WHERE run_id = NEW.run_id AND participation_status = 'joined');
    END IF;
    RETURN NULL;
END;
$$;
CREATE TRIGGER participant_zero_remaining AFTER UPDATE ON run_participants
FOR EACH ROW EXECUTE FUNCTION cancel_empty_deployment();

CREATE OR REPLACE FUNCTION publish_deployment(
    actor integer, project integer, snapshot integer, deployment_name text,
    summary text DEFAULT '', superseded_run integer DEFAULT NULL
) RETURNS integer LANGUAGE plpgsql AS $$
DECLARE new_id integer; previous_run deployment_runs;
BEGIN
    IF NOT has_project_permission(actor, project, 'edit_project') THEN
        RAISE EXCEPTION 'Project edit permission required';
    END IF;
    IF superseded_run IS NOT NULL THEN
        SELECT * INTO previous_run FROM deployment_runs WHERE id = superseded_run FOR UPDATE;
        IF NOT FOUND OR previous_run.project_id <> project OR previous_run.status <> 'pending' THEN
            RAISE EXCEPTION 'Only a pending deployment of the same project can be replaced';
        END IF;
        IF previous_run.snapshot_id = snapshot THEN RAISE EXCEPTION 'Capture a new snapshot for the replacement'; END IF;
        IF NULLIF(trim(summary), '') IS NULL THEN RAISE EXCEPTION 'Describe the published changes'; END IF;
    END IF;
    INSERT INTO deployment_runs (
        org_id, project_id, snapshot_id, name, change_summary, supersedes_run_id, created_by_user_id,
        dockerfile_path, compose_file_path, build_context_path, env_file_path, command_arguments,
        input_mount_destination, output_mount_destination
    ) SELECT org_id, id, snapshot, deployment_name, summary, superseded_run, actor,
             dockerfile_path, compose_file_path, build_context_path, env_file_path, command_arguments,
             input_mount_destination, output_mount_destination FROM projects WHERE id = project
    RETURNING id INTO new_id;
    INSERT INTO run_participants (run_id, user_id)
    SELECT new_id, m.user_id FROM user_organization m JOIN projects p ON p.org_id = m.org_id
    WHERE p.id = project AND has_project_permission(m.user_id, project, 'participate_in_deployment');
    IF superseded_run IS NOT NULL THEN
        INSERT INTO notifications (user_id, title, message)
        SELECT user_id, 'Deployment updated: ' || deployment_name,
            summary || ' Review and accept replacement deployment #' || new_id || '.'
        FROM run_participants WHERE run_id = superseded_run AND participation_status = 'joined';
        UPDATE deployment_runs SET status = 'skipped', ended_at = clock_timestamp(),
            terminal_reason = 'Replaced by deployment #' || new_id WHERE id = superseded_run;
    END IF;
    RETURN new_id;
END;
$$;

CREATE OR REPLACE FUNCTION join_deployment(actor integer, deployment integer, input_path text, output_path text)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE target deployment_runs;
BEGIN
    SELECT * INTO target FROM deployment_runs WHERE id = deployment FOR UPDATE;
    IF NOT FOUND OR target.status <> 'pending' THEN RAISE EXCEPTION 'Joining is only allowed while pending'; END IF;
    IF NOT has_project_permission(actor, target.project_id, 'participate_in_deployment') THEN
        RAISE EXCEPTION 'Participation permission required';
    END IF;
    INSERT INTO run_participants (run_id, user_id, participation_status, accepted_snapshot_id,
        accepted_at, joined_at, local_input_path, local_output_path)
    VALUES (deployment, actor, 'joined', target.snapshot_id, clock_timestamp(), clock_timestamp(), input_path, output_path)
    ON CONFLICT (run_id, user_id) DO UPDATE SET participation_status = 'joined',
        accepted_snapshot_id = EXCLUDED.accepted_snapshot_id, accepted_at = EXCLUDED.accepted_at,
        joined_at = EXCLUDED.joined_at, local_input_path = EXCLUDED.local_input_path,
        local_output_path = EXCLUDED.local_output_path, removed_at = NULL, removal_reason = NULL,
        stop_requested_at = NULL, stopped_at = NULL, execution_status = 'not_started';
END;
$$;

CREATE OR REPLACE FUNCTION start_deployment(actor integer, deployment integer)
RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    UPDATE deployment_runs SET status = 'started', started_by_user_id = actor, started_at = clock_timestamp()
    WHERE id = deployment AND status = 'pending';
    IF NOT FOUND THEN RAISE EXCEPTION 'Only a pending deployment can start'; END IF;
END;
$$;

CREATE OR REPLACE FUNCTION cancel_deployment(actor integer, deployment integer, reason text DEFAULT 'Cancelled by user')
RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM deployment_runs WHERE id = deployment
        AND has_project_permission(actor, project_id, 'start_deployment')) THEN
        RAISE EXCEPTION 'Start deployment permission required to cancel';
    END IF;
    UPDATE deployment_runs SET status = 'cancelled', cancelled_by_user_id = actor,
        ended_at = clock_timestamp(), terminal_reason = reason
    WHERE id = deployment AND status IN ('pending', 'started');
    IF NOT FOUND THEN RAISE EXCEPTION 'Only pending or started deployments can be cancelled'; END IF;
    UPDATE run_participants SET stop_requested_at = clock_timestamp()
    WHERE run_id = deployment AND participation_status = 'joined';
END;
$$;

CREATE OR REPLACE FUNCTION leave_deployment(actor integer, deployment integer)
RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    UPDATE run_participants SET participation_status = 'removed', removed_at = clock_timestamp(),
        removal_reason = 'voluntary', stop_requested_at = clock_timestamp()
    WHERE run_id = deployment AND user_id = actor AND participation_status IN ('pending', 'joined');
    IF NOT FOUND THEN RAISE EXCEPTION 'No current participation to leave'; END IF;
END;
$$;

-- Only a trusted system service may call this; no user-facing removal endpoint.
CREATE OR REPLACE FUNCTION system_remove_participant(deployment integer, participant integer, reason removal_reason)
RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    IF reason NOT IN ('permission_lost', 'unauthorized_behavior') THEN RAISE EXCEPTION 'System removal reason required'; END IF;
    UPDATE run_participants SET participation_status = 'removed', removed_at = clock_timestamp(),
        removal_reason = reason, stop_requested_at = clock_timestamp()
    WHERE run_id = deployment AND user_id = participant AND participation_status IN ('pending', 'joined');
END;
$$;

CREATE OR REPLACE FUNCTION refresh_project_participants(project integer) RETURNS void LANGUAGE plpgsql AS $$
DECLARE target deployment_runs;
BEGIN
    FOR target IN SELECT * FROM deployment_runs WHERE project_id = project AND status IN ('pending', 'started') ORDER BY id FOR UPDATE LOOP
        UPDATE run_participants SET participation_status = 'removed', removed_at = clock_timestamp(),
            removal_reason = 'permission_lost', stop_requested_at = clock_timestamp()
        WHERE run_id = target.id AND participation_status IN ('pending', 'joined')
          AND NOT has_project_permission(user_id, project, 'participate_in_deployment');
        IF target.status = 'pending' THEN
            INSERT INTO run_participants (run_id, user_id)
            SELECT target.id, m.user_id FROM user_organization m JOIN projects p ON p.org_id = m.org_id
            WHERE p.id = project AND has_project_permission(m.user_id, project, 'participate_in_deployment')
            ON CONFLICT (run_id, user_id) DO NOTHING;
        END IF;
    END LOOP;
END;
$$;

CREATE OR REPLACE FUNCTION refresh_membership_participation() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE project integer; old_org integer; new_org integer;
BEGIN
    IF TG_TABLE_NAME = 'organizations' THEN
        old_org := OLD.id;
        new_org := NEW.id;
    ELSE
        IF TG_OP <> 'INSERT' THEN old_org := OLD.org_id; END IF;
        IF TG_OP <> 'DELETE' THEN new_org := NEW.org_id; END IF;
    END IF;
    FOR project IN SELECT id FROM projects WHERE org_id IN (old_org, new_org) ORDER BY id LOOP
        PERFORM refresh_project_participants(project);
    END LOOP;
    RETURN NULL;
END;
$$;
CREATE TRIGGER membership_participation AFTER INSERT OR UPDATE OR DELETE ON user_organization
FOR EACH ROW EXECUTE FUNCTION refresh_membership_participation();
CREATE TRIGGER ownership_participation AFTER UPDATE OF owner_user_id ON organizations
FOR EACH ROW EXECUTE FUNCTION refresh_membership_participation();

CREATE OR REPLACE FUNCTION refresh_grant_participation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'INSERT' THEN PERFORM refresh_project_participants(OLD.project_id); END IF;
    IF TG_OP <> 'DELETE' THEN PERFORM refresh_project_participants(NEW.project_id); END IF;
    RETURN NULL;
END;
$$;
CREATE TRIGGER project_grant_participation AFTER INSERT OR UPDATE OR DELETE ON role_project_permission
FOR EACH ROW EXECUTE FUNCTION refresh_grant_participation();

REVOKE EXECUTE ON FUNCTION system_remove_participant(integer, integer, removal_reason) FROM PUBLIC;
