# API creation endpoints

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
