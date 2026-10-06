# Require two joined participants at deployment start

A deployment starts only when its starter has joined and at least one other participant has joined. Falling below two participants after execution starts does not automatically fail the deployment: a departed participant's earlier contribution can still affect the resulting model.

Joining and rejoining are restricted to pending deployments. During execution, participants may leave voluntarily or be removed by the system for permission loss or unauthorized behavior; other users cannot remove them. Users with permission to start deployments may cancel the whole deployment.

If no joined participants remain during execution, cancel the deployment and retain any previous contributions or partial result.
