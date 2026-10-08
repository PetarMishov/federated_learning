-- Run after schema.sql and populate.sql in an isolated test schema.
-- Every mutation is rolled back; violations must be rejected, not silently accepted.
\set ON_ERROR_STOP on
BEGIN;
CREATE FUNCTION expect_rejected(statement text, label text) RETURNS void LANGUAGE plpgsql AS $$
DECLARE rejected boolean := false;
BEGIN
    BEGIN EXECUTE statement;
    EXCEPTION WHEN OTHERS THEN rejected := true;
    END;
    IF NOT rejected THEN RAISE EXCEPTION 'Expected rejection: %', label; END IF;
END;
$$;

DO $$
DECLARE
    owner_id integer; alice integer; bob integer; charlie integer;
    project integer; snapshot integer; run integer; old_run integer; updated_run integer;
    collaborator_role integer; new_snapshot integer;
BEGIN
    SELECT id INTO owner_id FROM users WHERE username = 'demo';
    SELECT id INTO alice FROM users WHERE username = 'alice';
    SELECT id INTO bob FROM users WHERE username = 'bob';
    SELECT id INTO charlie FROM users WHERE username = 'charlie';
    SELECT id INTO project FROM projects WHERE name = 'Patient risk prediction';
    SELECT id INTO snapshot FROM snapshots WHERE project_id = project ORDER BY id LIMIT 1;
    SELECT id INTO collaborator_role FROM roles WHERE org_id = (SELECT org_id FROM projects WHERE id = project) AND name = 'Demo coordinator';

    IF (SELECT count(*) FROM deployment_runs) <> 25 OR (SELECT count(DISTINCT status) FROM deployment_runs) <> 6 THEN
        RAISE EXCEPTION 'Fixture population or rerun produced incorrect deployments';
    END IF;
    IF EXISTS (SELECT project_id FROM deployment_runs GROUP BY project_id
               HAVING count(*) < 4 OR count(DISTINCT status) < 3)
       OR (SELECT count(DISTINCT project_id) FROM deployment_runs) <> 6 THEN
        RAISE EXCEPTION 'Every demo project needs multiple deployments with varied states';
    END IF;
    IF (SELECT count(*) FROM snapshots) <> 7 OR (SELECT count(*) FROM users) <> 6 THEN
        RAISE EXCEPTION 'Fixture population or rerun duplicated snapshots/users';
    END IF;
    PERFORM expect_rejected(format('UPDATE snapshots SET git_commit_sha = %L WHERE id = %s', repeat('0', 40), snapshot), 'snapshot mutation');
    PERFORM expect_rejected(format('SELECT publish_deployment(%s, %s, %s, %L)', charlie, project, snapshot, 'Denied'), 'cross-organization publication');
    PERFORM expect_rejected(format('INSERT INTO role_permission(role_id, perm_id) SELECT %s, id FROM permissions WHERE name = %L', collaborator_role, 'edit_project'), 'project permission granted organization-wide');
    PERFORM expect_rejected(format('INSERT INTO role_project_permission(org_id,role_id,project_id,perm_id) SELECT p.org_id,%s,p.id,perm.id FROM projects p CROSS JOIN permissions perm WHERE p.name=%L AND perm.name=%L', collaborator_role, 'Federated learning benchmark', 'start_deployment'), 'cross-organization project grant');

    run := publish_deployment(owner_id, project, snapshot, 'Test start');
    PERFORM expect_rejected(format('SELECT start_deployment(%s,%s)', owner_id, run), 'starter has not joined');
    PERFORM join_deployment(alice, run, '/input/alice', '/output/alice');
    PERFORM join_deployment(bob, run, '/input/bob', '/output/bob');
    PERFORM expect_rejected(format('SELECT start_deployment(%s,%s)', owner_id, run), 'starter must join even if two others joined');
    PERFORM leave_deployment(bob, run);
    PERFORM expect_rejected(format('SELECT start_deployment(%s,%s)', alice, run), 'one joined participant');
    PERFORM join_deployment(owner_id, run, '/input/demo', '/output/demo');
    PERFORM start_deployment(owner_id, run);
    PERFORM expect_rejected(format('SELECT join_deployment(%s,%s,%L,%L)', bob, run, '/input', '/output'), 'late joining');
    PERFORM expect_rejected(format('UPDATE deployment_runs SET command_arguments=%L::jsonb WHERE id=%s', '{"epochs":99}', run), 'shared settings mutation');
    PERFORM expect_rejected(format('UPDATE deployment_runs SET snapshot_id=(SELECT max(id) FROM snapshots WHERE project_id=%s) WHERE id=%s', project, run), 'snapshot replacement after start');
    PERFORM leave_deployment(alice, run);
    IF (SELECT status FROM deployment_runs WHERE id = run) <> 'started' THEN RAISE EXCEPTION 'One remaining participant should continue'; END IF;
    PERFORM expect_rejected(format('SELECT join_deployment(%s,%s,%L,%L)', alice, run, '/input', '/output'), 'late rejoining');
    PERFORM leave_deployment(owner_id, run);
    IF (SELECT status FROM deployment_runs WHERE id = run) <> 'cancelled' THEN RAISE EXCEPTION 'Zero participants must cancel'; END IF;
    PERFORM expect_rejected(format('UPDATE deployment_runs SET status=%L,ended_at=NULL WHERE id=%s', 'pending', run), 'terminal restart');

    old_run := publish_deployment(owner_id, project, snapshot, 'Test publication');
    PERFORM join_deployment(owner_id, old_run, '/input/demo', '/output/demo');
    PERFORM join_deployment(alice, old_run, '/input/alice', '/output/alice');
    SELECT id INTO new_snapshot FROM snapshots WHERE project_id = project ORDER BY id DESC LIMIT 1;
    updated_run := publish_deployment(owner_id, project, new_snapshot, 'Test replacement', 'Changed example settings', old_run);
    IF (SELECT status FROM deployment_runs WHERE id=old_run) <> 'skipped'
       OR (SELECT count(*) FROM run_participants WHERE run_id=old_run AND participation_status='joined') <> 2
       OR EXISTS (SELECT 1 FROM run_participants WHERE run_id=updated_run AND participation_status <> 'pending')
       OR (SELECT count(*) FROM notifications WHERE message LIKE '%replacement deployment #' || updated_run || '.%') <> 2 THEN
        RAISE EXCEPTION 'Replacement must retain history, reset acceptance, and notify joined users';
    END IF;
    PERFORM join_deployment(alice, updated_run, '/input/alice', '/output/alice');
    PERFORM leave_deployment(alice, updated_run);
    PERFORM join_deployment(alice, updated_run, '/input/alice', '/output/alice');
    IF (SELECT participation_status FROM run_participants WHERE run_id=updated_run AND user_id=alice) <> 'joined' THEN
        RAISE EXCEPTION 'Removed participants can rejoin while pending';
    END IF;

    PERFORM join_deployment(owner_id, updated_run, '/input/demo', '/output/demo');
    PERFORM start_deployment(owner_id, updated_run);
    -- Removing a project-specific participation grant must remove the affected
    -- participant and request local stop, without failing the remaining execution.
    DELETE FROM role_project_permission WHERE role_id=collaborator_role AND project_id=project
        AND perm_id=(SELECT id FROM permissions WHERE name='participate_in_deployment');
    IF (SELECT participation_status FROM run_participants WHERE run_id=updated_run AND user_id=alice) <> 'removed'
       OR (SELECT removal_reason FROM run_participants WHERE run_id=updated_run AND user_id=alice) <> 'permission_lost'
       OR (SELECT stop_requested_at FROM run_participants WHERE run_id=updated_run AND user_id=alice) IS NULL
       OR (SELECT status FROM deployment_runs WHERE id=updated_run) <> 'started' THEN
        RAISE EXCEPTION 'Permission revocation must remove participation while preserving viable execution';
    END IF;
    PERFORM expect_rejected(format('SELECT cancel_deployment(%s,%s)', charlie, updated_run), 'unauthorized cancellation');
    PERFORM cancel_deployment(owner_id, updated_run);
    SET CONSTRAINTS ALL IMMEDIATE;
END;
$$;
ROLLBACK;
