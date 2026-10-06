# Replace pending deployments when updates are published

Publishing changed project files or shared execution settings creates a new pending deployment and snapshot, and marks the previous pending deployment as skipped. This preserves the exact version and participation history that users previously accepted, rather than changing that version in place.

Eligible participants must accept the replacement deployment again, and previously joined users are notified of the changes. Working-copy edits are unpublished until an explicit update action; changes to a participant's local input and output paths do not invalidate shared acceptance. Once execution starts, shared changes require a separate deployment.
