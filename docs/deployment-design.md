# Deployment model

This design records the agreed deployment model. SQL constraints, triggers, and transactional lifecycle functions implement the database rules; HTTP deployment endpoints and execution orchestration are future work.

## Lifecycle

Deployments have six states: pending, started, finished, cancelled, failed, and skipped. Finished means successful completion; failed means unsuccessful execution. There is no paused state.

Pending deployments can start, be cancelled, or be superseded and skipped. Started deployments can finish, fail, or be cancelled. Finished, failed, cancelled, and skipped are terminal states.

The starter must have joined, retain current organization membership and the project's start and participation permissions, and have at least one other joined participant. Joining and rejoining close when execution starts. A later drop below two participants does not automatically fail execution.

## Publication and acceptance

Creating a pending deployment captures the project's exact files into a snapshot. Working-copy edits remain unpublished until an explicit update action. Publishing changed files or shared execution settings creates a new snapshot and replacement pending deployment, marks the old deployment skipped, and retains the old snapshot and participation history.

Currently eligible participants must accept the replacement deployment again. Previously joined users are notified of the published changes and the replacement. A participant changing local input or output paths does not invalidate shared acceptance. Shared code and execution settings cannot change once a deployment starts.

Each snapshot belongs to one project, each project belongs to one organization, and each deployment references a snapshot of its own project. Replacement deployments must remain in the same project and organization.

## Participation

Participation states are pending, joined, and removed. Pending means currently eligible but not yet accepted. Joined means accepted participation in this deployment. Removed covers both voluntary departure and system removal, with a retained reason.

An eligible removed user may rejoin while the deployment is pending. During execution, users can leave voluntarily or be removed by the system for membership loss, participation-permission loss, or unauthorized behavior. Other users cannot remove a participant mid-execution. Unauthorized-behavior detection is a future feature.

Users with the project's start permission, including organization owners, may cancel pending or started deployments. Historical participation survives membership changes.

## Permissions and storage

Managing roles is an organization permission. Editing projects, starting deployments, and participating in deployments are granted separately for each project. Organization owners retain all permissions in their organization.

Snapshot files live in each project’s private bare Git repository, separately from editable working copies and outside frontend assets. The database retains the stored commit ID, source provenance, and ownership metadata. Snapshot comparison does not require a GitHub or GitLab hosting service.

## Implementation scope

Maintain the current development schema, database diagram, and demo permissions, snapshots, deployments, and participation states. Publish real demo Git commits and snapshot refs before recording their metadata. Keep existing user, organization, project, and member API responses compatible; make only necessary small backend or frontend changes.

Execution orchestration, repository connectors, file editing, and unauthorized-behavior detection remain future features. Model the agreed rules and document the runtime operations that must enforce them.

## No remaining participants

If every joined participant leaves or is removed after execution starts, the deployment is cancelled with a no-participants reason. Earlier contributions and partial results remain valid; dropping to one participant does not automatically fail execution.

## Local development

Database files live in `db/`; see [the database README](../db/README.md). On an empty database, run `./db/scripts/setup_db.sh`, then `./db/scripts/populate_db.sh`. Setup loads `schema.sql`, which includes `deployment_rules.sql`. On an already initialized database, population can be rerun safely. Development uses one current schema.

Snapshots live in `storage/git/projects/<project-id>.git`. An absolute
`GIT_STORAGE_DIR` in `.env` overrides the Git root. Each snapshot is retained at
`refs/snapshots/<snapshot-id>` and its database record contains `git_commit_sha`.
The population script builds deterministic Git commits from the tracked fixtures,
verifies existing refs rather than replacing them, and publishes refs before
committing metadata. Download archives will be generated from saved commits.

Future import/download handlers must reject traversal, exclude Git metadata and
local datasets from uploaded file trees, and authorize access against current
project permissions. Resolve files at the saved commit rather than a moving branch.
The snapshot list, metadata, directory, and text file routes are implemented with
authentication and project access checks. File reads resolve exact paths at the
stored commit. Other snapshot routes return `501 Not Implemented`; their handlers are
placeholders. The current Git helper imports only API-owned local repositories.

## Database operations

- `publish_deployment`: verified actor, project/snapshot, name, summary, and optional predecessor. Copies current project defaults into an immutable deployment, creates eligible pending participation, and atomically replaces/notifies when publishing an update. It expects an already captured snapshot. Update project defaults first in the same transaction if publishing changed shared settings.
- `join_deployment`: records the actor's snapshot acceptance and local input/output paths while pending, including rejoining.
- `start_deployment`: starts a pending deployment after checking the starter's acceptance, project permissions, and at least one additional joined participant.
- `cancel_deployment`: cancels pending/started deployments with a verified permitted actor and requests local stop.
- `leave_deployment`: voluntary removal of the authenticated actor's own participation.
- `system_remove_participant`: trusted system-only removal for permission loss or unauthorized behavior; no user-facing removal endpoint exists.

Membership and participation-grant changes reconcile current participation automatically. This records removal and stop requests; it cannot stop a worker process itself. A future coordinator must reject contributions from users whose current authorization was revoked and handle stop requests/acknowledgments. It must also authenticate system completion/failure updates and arrange partial-result retention outside these tables.

Runtime services must derive actor IDs from verified sessions, authorize grant edits, and restrict direct lifecycle writes. They should retry transactions on PostgreSQL serialization/deadlock errors; grant changes and lifecycle operations serialize through deployment locks. The existing development database role is a schema owner, so it must not be exposed as an end-user database connection.
