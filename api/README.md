# API endpoints

API startup loads the private Git SSH identity into `AppState.git`. Run the
[Git server setup](../git-server/README.md) first; existing HTTP endpoints retain
their current authentication and behavior.

## Project members

`GET /projects/{proj_id}/members` requires a valid bearer token and returns:

```json
{"members": [{"id": 1, "username": "alice", "role_id": 2, "role_name": "Coordinator"}]}
```

Members are the organization owner plus current organization members whose roles
have at least one permission on this project. Users appear once even when their
role grants multiple permissions, sorted by username and ID. Role fields may be
null, including for an owner with no assigned role. This list is separate from
deployment participation.

Any current member of the project's organization can read the list. A caller
outside that organization, or a nonexistent project, receives an empty list,
matching the existing list endpoints. Invalid or revoked tokens return `401`;
database failures return `500`.

The frontend loads this list when the project page's Members drawer opens,
refreshes it on reopening, and offers retry on errors. The development proxy
forwards `/projects/{id}/members` to the API.

## Creation

Both endpoints require `Authorization: Bearer <token>` and
`Content-Type: application/json`. Ownership and creator IDs come from the verified
session; request bodies contain only a name.

| Endpoint | Access | Result |
| --- | --- | --- |
| `POST /organizations` | Any authenticated user | Organization owned by the caller, with owner membership |
| `POST /organizations/{org_id}/projects` | Organization owner | Project belonging to that organization |

For either endpoint, submit:

```json
{"name": "Research lab"}
```

Names are trimmed and must contain 1–255 characters, with no internal control
characters. Additional fields are rejected. Names need not be unique under the
current database schema.

Successful creation returns `201 Created` and the created object:

```json
{"id": 1, "name": "Research lab", "owner_user_id": 1}
```

```json
{"id": 1, "org_id": 1, "created_by_user_id": 1, "name": "Training"}
```

The organization and its owner's membership are inserted in one transaction.
Project creation checks and locks current ownership and membership while
inserting. New projects use `/data/input` and `/data/output` as container mount
destinations and `.` as the build context, matching existing demo defaults.
Creation does not publish a snapshot or start a deployment.

Missing, invalid, or revoked tokens return `401`. Invalid names return `400`;
malformed JSON returns `400`, and missing, unknown, or incorrectly typed fields
return `422`. Missing JSON content type returns `415`. Project creation returns
`403` when the caller is not the organization owner or the organization does not
exist. Database failures return `500` without exposing database details.

The current permission catalog has no organization-level project-creation grant.
Project creation therefore uses an owner-only policy; role-management permission
does not grant it.

Run regular tests from `api/` with `cargo test`. The creation integration test
uses its own temporary schema, leaves existing application data untouched, and
requires a database account that can create schemas:

```bash
TEST_DATABASE_URL='<postgres connection URL>' cargo test \
  creation_endpoints_persist_membership_and_enforce_project_ownership -- --ignored
```
